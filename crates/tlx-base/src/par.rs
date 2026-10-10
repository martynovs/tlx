//! Cooperative early stop for the auxiliary thread pools (the liveness
//! expansion and the resume-point transcoding): when one worker panics, the
//! others stop instead of running to the end of the pool before the panic is
//! re-raised.
//!
//! A panic is printed by the panic hook where it happens; the join below
//! re-raises the payload without printing again (`resume_unwind`).

use parking_lot::{Condvar, Mutex};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub fn on_panic<R>(hook: impl FnOnce(), f: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(payload) => {
            hook();
            resume_unwind(payload)
        }
    }
}

/// Set when a worker of a pool panicked; the other workers poll
/// [`Abort::requested`] and return early.
#[derive(Default)]
pub struct Abort(AtomicBool);

impl Abort {
    pub fn new() -> Abort {
        Abort(AtomicBool::new(false))
    }

    /// Whether another worker of the pool panicked (stop as soon as
    /// convenient).
    #[inline]
    pub fn requested(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Request the abort (a worker panicked).
    pub fn set(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Run a worker body: if it panics, request the abort (so the other
    /// workers stop early) and let the panic go on, to be re-raised by
    /// [`join_pool`] once every handle is joined.
    pub fn guard<R>(&self, f: impl FnOnce() -> R) -> R {
        on_panic(|| self.set(), f)
    }
}

/// Join every handle (the abort flag makes the other workers stop early),
/// then re-raise the first panic among them.
pub fn join_pool<T>(handles: Vec<std::thread::ScopedJoinHandle<'_, T>>) -> Vec<T> {
    join_pool_with(handles, || {})
}

pub fn join_pool_with<T>(handles: Vec<std::thread::ScopedJoinHandle<'_, T>>, before_panic: impl FnOnce()) -> Vec<T> {
    let mut outs = Vec::with_capacity(handles.len());
    let mut panic = None;
    for h in handles {
        match h.join() {
            Ok(o) => outs.push(o),
            Err(p) => {
                panic.get_or_insert(p);
            }
        }
    }
    if let Some(p) = panic {
        before_panic();
        resume_unwind(p);
    }
    outs
}

#[derive(Clone, Copy)]
pub enum Stack {
    Default,
    Eval,
}

impl Stack {
    fn spawn<'scope, 'env, T: Send + 'scope>(
        self,
        scope: &'scope std::thread::Scope<'scope, 'env>,
        f: impl FnOnce() -> T + Send + 'scope,
    ) -> std::thread::ScopedJoinHandle<'scope, T> {
        match self {
            Self::Default => crate::stack::spawn_default(scope, f),
            Self::Eval => crate::stack::spawn_eval(scope, f),
        }
    }
}

pub struct Pool {
    pub threads: usize,
    pub stack: Stack,
}

impl Pool {
    pub fn map_owned<A: Send, B: Send, E: Send>(&self, parts: Vec<A>, f: impl Fn(&Abort, A) -> Result<B, E> + Sync) -> Result<Vec<B>, E> {
        let chunk = parts.len().div_ceil(self.threads.max(1)).max(1);
        let mut parts = parts.into_iter();
        let abort = Abort::new();
        let (abort, f) = (&abort, &f);
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            loop {
                let group: Vec<_> = parts.by_ref().take(chunk).collect();
                if group.is_empty() {
                    break;
                }
                handles.push(self.stack.spawn(scope, move || {
                    abort.guard(|| {
                        let mut results = Vec::with_capacity(group.len());
                        for part in group {
                            results.push(f(abort, part));
                        }
                        results
                    })
                }));
            }
            join_pool(handles).into_iter().flatten().collect()
        })
    }

    pub fn map_chunks<S: Sync, T: Send, E: Send>(
        &self,
        items: &[S],
        chunk: usize,
        f: impl Fn(&Abort, &[S]) -> Result<Vec<T>, E> + Sync,
    ) -> Result<Vec<T>, E> {
        assert!(chunk > 0);
        let next = AtomicUsize::new(0);
        let workers = self.threads.max(1).min(items.len().div_ceil(chunk));
        let parts = self.map_owned((0..workers).collect(), |abort, _| {
            let mut mine = Vec::new();
            while !abort.requested() {
                let start = next.fetch_add(chunk, Ordering::Relaxed);
                if start >= items.len() {
                    break;
                }
                mine.push((start, f(abort, &items[start..items.len().min(start.saturating_add(chunk))])?));
            }
            Ok(mine)
        })?;
        let mut parts: Vec<_> = parts.into_iter().flatten().collect();
        parts.sort_by_key(|(start, _)| *start);
        Ok(parts.into_iter().flat_map(|(_, part)| part).collect())
    }

    pub fn for_each_part(&self, f: impl Fn(&Abort, usize) + Sync) {
        let abort = Abort::new();
        let (abort, f) = (&abort, &f);
        std::thread::scope(|scope| {
            let handles = (1..self.threads.max(1)).map(|part| self.stack.spawn(scope, move || abort.guard(|| f(abort, part)))).collect();
            let caller = catch_unwind(AssertUnwindSafe(|| abort.guard(|| f(abort, 0))));
            let joined = catch_unwind(AssertUnwindSafe(|| join_pool(handles)));
            if let Err(payload) = caller {
                resume_unwind(payload);
            }
            if let Err(payload) = joined {
                resume_unwind(payload);
            }
        });
    }
}

/// A stop flag for periodic helpers: setting it wakes a sleeping [`every`]
/// at once instead of after the rest of its period.
#[derive(Default)]
pub struct Done {
    flag: AtomicBool,
    lock: Mutex<()>,
    wake: Condvar,
}

impl Done {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self) {
        self.flag.store(true, Ordering::Release);
        // (Taking the lock orders the store before a waiter's re-check.)
        drop(self.lock.lock());
        self.wake.notify_all();
    }

    pub fn is_set(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    /// Sleep up to `period`; true when [`Done::set`] ended it (or came before).
    fn sleep(&self, period: Duration) -> bool {
        let deadline = std::time::Instant::now() + period;
        let mut guard = self.lock.lock();
        while !self.is_set() {
            if self.wake.wait_until(&mut guard, deadline).timed_out() {
                break;
            }
        }
        self.is_set()
    }
}

/// Call `f` every `period` until `done` is set; returns promptly then,
/// without a last call.
pub fn every(period: Duration, done: &Done, mut f: impl FnMut()) {
    while !done.is_set() {
        if done.sleep(period) {
            break;
        }
        f();
    }
}

pub struct OneShot<R, S = ()> {
    state: Mutex<(S, Option<R>)>,
    ready: Condvar,
}

impl<R> Default for OneShot<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> OneShot<R> {
    pub fn new() -> Self {
        Self::with_state(())
    }
}

impl<R, S> OneShot<R, S> {
    pub fn with_state(state: S) -> Self {
        Self { state: Mutex::new((state, None)), ready: Condvar::new() }
    }

    pub fn set(&self, result: R) {
        self.complete(|_| result);
    }

    pub fn complete(&self, f: impl FnOnce(&mut S) -> R) {
        let mut state = self.state.lock();
        state.1 = Some(f(&mut state.0));
        self.ready.notify_all();
    }

    pub fn access<R2>(&self, f: impl FnOnce(&mut S) -> R2) -> R2 {
        f(&mut self.state.lock().0)
    }

    pub fn try_get(&self) -> Option<R>
    where
        R: Clone,
    {
        self.state.try_lock().and_then(|state| state.1.clone())
    }

    pub fn ready(&self) -> bool {
        self.state.lock().1.is_some()
    }

    pub fn try_take(&self) -> Option<R> {
        self.state.lock().1.take()
    }

    pub fn wait(&self) -> R
    where
        R: Clone,
    {
        let mut state = self.state.lock();
        loop {
            if let Some(result) = &state.1 {
                return result.clone();
            }
            self.ready.wait(&mut state);
        }
    }

    pub fn take(&self) -> R {
        let mut state = self.state.lock();
        loop {
            if let Some(result) = state.1.take() {
                return result;
            }
            self.ready.wait(&mut state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[global_allocator]
    static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

    #[test]
    fn ordered_maps_keep_all_results_and_first_error() {
        let pool = Pool { threads: 3, stack: Stack::Default };
        let values: Vec<_> = (0..131).collect();
        let mapped = pool.map_chunks(&values, 7, |_, chunk| Ok::<_, ()>(chunk.iter().map(|value| value * 2).collect())).unwrap();
        assert_eq!(mapped, values.iter().map(|value| value * 2).collect::<Vec<_>>());
        assert_eq!(pool.map_owned(values.clone(), |_, value| Ok::<_, ()>(value * 2)).unwrap(), mapped);
        let visited = AtomicUsize::new(0);
        let error = pool.map_owned(vec![0, 1, 2, 3], |_, value| {
            visited.fetch_add(1, Ordering::Relaxed);
            if value == 0 || value == 3 {
                Err(value)
            } else {
                Ok(value)
            }
        });
        assert_eq!(error, Err(0));
        assert_eq!(visited.load(Ordering::Relaxed), 4);
        assert_eq!(pool.map_owned::<usize, usize, ()>(Vec::new(), |_, value| Ok(value)), Ok(Vec::new()));
        assert_eq!(pool.map_chunks::<usize, usize, ()>(&[], 1, |_, _| unreachable!()), Ok(Vec::new()));
    }

    #[test]
    fn chunk_pool_stops_and_preserves_worker_payload() {
        let pool = Pool { threads: 2, stack: Stack::Eval };
        let started = std::sync::Barrier::new(2);
        let stopped = AtomicBool::new(false);
        let result = catch_unwind(AssertUnwindSafe(|| {
            pool.map_chunks::<_, usize, ()>(&[0, 1, 2, 3], 1, |abort, chunk| {
                started.wait();
                if chunk[0] == 0 {
                    std::panic::panic_any(123usize);
                }
                while !abort.requested() {
                    std::thread::yield_now();
                }
                stopped.store(true, Ordering::Relaxed);
                Ok(Vec::new())
            })
        }));
        assert_eq!(*result.unwrap_err().downcast::<usize>().unwrap(), 123);
        assert!(stopped.load(Ordering::Relaxed));
    }

    #[test]
    fn part_zero_stays_on_caller() {
        let pool = Pool { threads: 4, stack: Stack::Default };
        let caller = std::thread::current().id();
        pool.for_each_part(|_, index| {
            assert_eq!(std::thread::current().id() == caller, index == 0);
        });
    }

    #[test]
    fn owned_map_can_repack_parts_on_fresh_workers() {
        let pool = Pool { threads: 3, stack: Stack::Default };
        let parts = vec![[1u64; 8], [2; 8], [3; 8]];
        assert_eq!(pool.map_owned(parts, |_, part| Ok::<_, ()>([part[0]; 4])).unwrap(), vec![[1; 4], [2; 4], [3; 4]]);
    }

    #[test]
    fn join_hook_runs_after_every_worker() {
        let done = AtomicBool::new(false);
        let hook = AtomicBool::new(false);
        let result = catch_unwind(AssertUnwindSafe(|| {
            std::thread::scope(|scope| {
                let handles = vec![scope.spawn(|| panic!("first")), scope.spawn(|| done.store(true, Ordering::Relaxed))];
                join_pool_with(handles, || {
                    assert!(done.load(Ordering::Relaxed));
                    hook.store(true, Ordering::Relaxed);
                });
            })
        }));
        assert_eq!(result.unwrap_err().downcast_ref::<&str>(), Some(&"first"));
        assert!(hook.load(Ordering::Relaxed));
    }

    #[test]
    fn periodic_monitor_keeps_the_final_tick() {
        let done = Done::new();
        let mut ticks = 0;
        every(Duration::ZERO, &done, || {
            ticks += 1;
            done.set();
        });
        every(Duration::ZERO, &done, || panic!("already done"));
        assert_eq!(ticks, 1);
    }

    #[test]
    fn periodic_monitor_wakes_promptly_when_done() {
        let done = Done::new();
        let start = std::time::Instant::now();
        std::thread::scope(|sc| {
            sc.spawn(|| {
                std::thread::sleep(Duration::from_millis(20));
                done.set();
            });
            every(Duration::from_secs(30), &done, || panic!("no tick within the period"));
        });
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn oneshot_wakes_readers_and_can_be_consumed() {
        let result = OneShot::new();
        assert_eq!(result.try_get(), None);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..3).map(|_| scope.spawn(|| result.wait())).collect();
            result.set(42);
            assert_eq!(join_pool(handles), vec![42; 3]);
        });
        assert!(result.ready());
        assert_eq!(result.take(), 42);
        assert!(!result.ready());
        assert_eq!(result.try_take(), None);
        let result = OneShot::new();
        std::thread::scope(|scope| {
            let handle = scope.spawn(|| result.take());
            result.set(Box::new(7));
            assert_eq!(*handle.join().unwrap(), 7);
        });
    }

    #[test]
    fn oneshot_completion_keeps_data_and_result_under_one_lock() {
        let result = OneShot::with_state(Some(vec![1, 2]));
        result.complete(|data| {
            assert!(result.try_get().is_none());
            assert_eq!(data.take(), Some(vec![1, 2]));
            Ok::<_, usize>(7)
        });
        assert_eq!(result.wait(), Ok(7));
        assert!(result.access(|data| data.is_none()));
        let failed = OneShot::with_state(Some(vec![3]));
        failed.complete(|_| Err::<usize, _>(9));
        assert_eq!(failed.wait(), Err(9));
        assert_eq!(failed.access(Option::take), Some(vec![3]));
    }

    /// A panicking worker body sets the abort flag (the other workers poll
    /// it) and the panic is re-raised.
    #[test]
    fn guard_sets_the_flag_on_panic() {
        let abort = Abort::new();
        let r = catch_unwind(AssertUnwindSafe(|| abort.guard(|| panic!("injected worker panic"))));
        assert_eq!(r.unwrap_err().downcast_ref::<&str>().copied(), Some("injected worker panic"));
        assert!(abort.requested());
    }

    /// The flag stays clear when the body completes.
    #[test]
    fn guard_clears_nothing_on_success() {
        let abort = Abort::new();
        assert_eq!(abort.guard(|| 7), 7);
        assert!(!abort.requested());
    }

    /// The join re-raises the panic of a worker once every handle is joined
    /// (never swallowing it), and returns the results when there is none.
    #[test]
    fn join_pool_re_raises_the_first_panic() {
        let r = catch_unwind(AssertUnwindSafe(|| {
            std::thread::scope(|sc| {
                let hs = vec![sc.spawn(|| 1), sc.spawn(|| panic!("injected worker panic")), sc.spawn(|| 3)];
                join_pool(hs)
            })
        }));
        assert_eq!(r.unwrap_err().downcast_ref::<&str>().copied(), Some("injected worker panic"));
    }

    #[test]
    fn join_pool_returns_results_without_panics() {
        let r: Vec<usize> = std::thread::scope(|sc| {
            let hs = vec![sc.spawn(|| 0), sc.spawn(|| 2), sc.spawn(|| 4), sc.spawn(|| 6)];
            join_pool(hs)
        });
        assert_eq!(r, vec![0, 2, 4, 6]);
    }
}
