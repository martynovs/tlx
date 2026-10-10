//! Side effects of evaluation (Print/PrintT output, files written by
//! CSVWrite/JsonSerialize, TLCSet registers, programs run by IOExec) across the restarts
//! of a compiled-tier run.
//!
//! The compiled tier restarts a search (or a simulation) after a layout
//! miss: either it resumes at the start of the level it was expanding, or it
//! starts over. Either way it evaluates again what it evaluated since that
//! point, and the run must show the effects of one uninterrupted run:
//! - output: a line printed since the restart point is *owed*: when the
//!   restarted run evaluates that line again it is not printed a second
//!   time (lines are matched by content, as a multiset; with one worker the
//!   restarted run prints them in the same order, so the output is that of
//!   an uninterrupted run);
//! - files: appends restore the previous length; overwrites restore the
//!   previous bytes; files created since the restart point are removed;
//! - programs (IOExec and its variants): a run since the restart point
//!   cannot be undone; its result is *owed*: when the restarted run
//!   evaluates the same command again (same program, arguments and
//!   environment), it gets that result instead of running the program a
//!   second time (results are matched by command, in the order they were
//!   obtained; with one worker the restarted run evaluates them in the same
//!   order). The compiled tier's pre-run is rolled back the same way, so
//!   the commands it ran are not run again by the search (the pre-run's
//!   states are a prefix of the search's);
//! - registers: a resumed worker starts from the registers it had when it
//!   began the interrupted level (or ended, if it did not take part in that
//!   level); a search started over starts from the main thread's registers
//!   (restored by the driver).
//!
//! The journal records while a restart is possible (`set_active`); the
//! explorer marks the start of every level per worker (`mark`) and the end
//! of each worker (`worker_end`).

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tlx_value::Value;

use crate::error::{EResult, EvalError};
use crate::natives::PreparedCommand;

/// TLCGet/TLCSet registers (looked up on every call: Fx, not SipHash).
pub(crate) type Registers = tlx_base::hash::FxMap<i64, Value>;

#[derive(Default)]
pub(crate) struct Effects {
    /// Recording: a restart of the run is possible.
    active: AtomicBool,
    /// Lines are owed (`Journal::owed` is not empty).
    owing: AtomicBool,
    /// Command results are owed (`Journal::owed_runs` is not empty).
    owing_runs: AtomicBool,
    journal: Mutex<Journal>,
}

type MemoUndo = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Journal {
    /// Lazy memo entries produced while this run may be rolled back.
    memos: Vec<MemoUndo>,
    /// Exact lines printed since the run started (from scratch),
    /// with their numbers of occurrences.
    printed: ReplayMap<Arc<str>, u64>,
    /// Lines printed whose evaluation was rolled back: not printed again.
    owed: ReplayMap<Arc<str>, u64>,
    /// Files before their first write in the run.
    files: HashMap<PathBuf, FileUndo>,
    /// Orders first writes across workers (length is not a time ordering).
    file_order: u64,
    /// The results of commands run since the run started, by exact key,
    /// in order.
    ran: ReplayMap<Arc<PreparedCommand>, Vec<Value>>,
    /// Results of commands whose evaluation was rolled back: the next
    /// evaluations of the command get them (in order) instead of running it.
    owed_runs: ReplayMap<Arc<PreparedCommand>, VecDeque<Value>>,
    /// Per worker, since the start of the level it is expanding.
    workers: Vec<WorkerJournal>,
    /// The registers each worker of a resumed search starts from.
    resume: Vec<Option<Registers>>,
}

#[derive(Default)]
struct WorkerJournal {
    memos: Vec<MemoUndo>,
    /// Exact lines it evaluated (printed or owed).
    lines: Vec<Arc<str>>,
    files: HashMap<PathBuf, FileUndo>,
    /// The commands it evaluated (run or owed) and their results, in order.
    runs: Vec<(Arc<PreparedCommand>, Value)>,
    /// Its registers when the level started, and when it ended.
    mark: Option<Registers>,
    end: Option<Registers>,
}

// Inject a constant hasher into the real journal maps in tests: every
// distinct key collides, so replay must depend on equality, never a digest.
#[cfg(not(test))]
type ReplayMap<K, V> = HashMap<K, V>;
#[cfg(test)]
type ReplayMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<tests::ConstantHasher>>;

/// Every file writer must declare what it can change before writing.
#[derive(Clone, Copy)]
pub(crate) enum FileWrite {
    /// Extends the file without changing its existing bytes.
    Append,
    /// May truncate or replace existing bytes.
    Overwrite,
}

#[derive(Clone)]
enum FileBefore {
    Missing,
    Length(u64),
    Bytes(Vec<u8>),
}

struct FileUndo {
    order: u64,
    before: FileBefore,
}

impl FileBefore {
    /// Upgrade an append checkpoint before the first destructive write.
    /// Any bytes appended since that checkpoint are excluded.
    fn preserve_bytes(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        if let Self::Length(len) = self {
            let prefix = bytes
                .get(..usize::try_from(*len).unwrap_or(usize::MAX))
                .ok_or_else(|| std::io::Error::other("file shrank outside the effect journal"))?;
            *self = Self::Bytes(prefix.to_vec());
        }
        Ok(())
    }

    fn restore(&self, path: &std::path::Path) -> std::io::Result<()> {
        let result = match self {
            Self::Missing => match std::fs::remove_file(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                r => r,
            },
            Self::Length(n) => std::fs::OpenOptions::new().write(true).open(path).and_then(|f| f.set_len(*n)),
            Self::Bytes(bytes) => std::fs::write(path, bytes),
        };
        result.map_err(|e| std::io::Error::new(e.kind(), format!("cannot restore file {}: {e}", path.display())))
    }
}

impl Journal {
    fn worker(&mut self, w: usize) -> &mut WorkerJournal {
        if self.workers.len() <= w {
            self.workers.resize_with(w + 1, Default::default);
        }
        &mut self.workers[w]
    }
}

impl Effects {
    /// Whether undo records are kept (`set_active`).
    pub(crate) fn recording(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub(crate) fn set_active(&self, on: bool) {
        self.active.store(on, Ordering::SeqCst);
    }

    /// Print `line` (to stderr), unless it is owed. `worker`: the worker
    /// evaluating it (None: the main thread).
    pub(crate) fn print(&self, line: &str, worker: Option<usize>) {
        if !self.active.load(Ordering::Relaxed) && !self.owing.load(Ordering::Relaxed) {
            eprintln!("{line}");
            return;
        }
        let key: Arc<str> = Arc::from(line);
        let mut j = self.journal.lock().unwrap();
        let active = self.active.load(Ordering::Relaxed);
        if let (true, Some(w)) = (active, worker) {
            // (Only workers that mark levels can be rolled back to one.)
            let wj = j.worker(w);
            if wj.mark.is_some() {
                wj.lines.push(key.clone());
            }
        }
        if let Some(n) = j.owed.get_mut(line) {
            *n -= 1;
            if *n == 0 {
                j.owed.remove(line);
                self.owing.store(!j.owed.is_empty(), Ordering::Relaxed);
            }
            return;
        }
        // (Printed under the lock: owed lines are matched in output order.)
        eprintln!("{line}");
        if active {
            *j.printed.entry(key).or_default() += 1;
        }
    }

    /// The result of the command with exact `key` evaluated by `worker`: an owed
    /// result if there is one, otherwise `run()` (whose result is recorded
    /// while a restart is possible).
    pub(crate) fn run_command(&self, key: &PreparedCommand, worker: Option<usize>, run: impl FnOnce() -> EResult<Value>) -> EResult<Value> {
        if !self.active.load(Ordering::Relaxed) && !self.owing_runs.load(Ordering::Relaxed) {
            return run();
        }
        // The prepared key contains only owned strings; equality and hashing
        // cannot force lazy values or enter the effect journal again.
        let key = Arc::new(key.clone());
        let owed = {
            let mut j = self.journal.lock().unwrap();
            let v = j.owed_runs.get_mut(&key).and_then(VecDeque::pop_front);
            if v.is_some() && j.owed_runs.get(&key).is_some_and(VecDeque::is_empty) {
                j.owed_runs.remove(&key);
                self.owing_runs.store(!j.owed_runs.is_empty(), Ordering::Relaxed);
            }
            v
        };
        let fresh = owed.is_none();
        // (The program runs outside the lock.)
        let v = match owed {
            Some(v) => v,
            None => run()?,
        };
        if self.active.load(Ordering::Relaxed) {
            let mut j = self.journal.lock().unwrap();
            if fresh {
                j.ran.entry(key.clone()).or_default().push(v.clone());
            }
            if let Some(w) = worker {
                let wj = j.worker(w);
                if wj.mark.is_some() {
                    wj.runs.push((key, v.clone()));
                }
            }
        }
        Ok(v)
    }

    /// Record the state before a write. Append may only extend a file;
    /// overwrite may replace any bytes. Failure to preserve the checkpoint
    /// rejects the write; failure to restore it must stop the restarted run.
    /// The callback performs only the final I/O: evaluating native values
    /// under this lock could enter the journal again through nested effects.
    pub(crate) fn file_write<R>(
        &self,
        path: &str,
        worker: Option<usize>,
        kind: FileWrite,
        write: impl FnOnce() -> EResult<R>,
    ) -> EResult<R> {
        if !self.active.load(Ordering::Relaxed) {
            return write();
        }
        let path = PathBuf::from(path);
        let mut j = self.journal.lock().unwrap();
        let mut preserve = || -> std::io::Result<()> {
            let before = match std::fs::metadata(&path) {
                Ok(m) => match kind {
                    FileWrite::Append => FileBefore::Length(m.len()),
                    FileWrite::Overwrite => FileBefore::Bytes(std::fs::read(&path)?),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => FileBefore::Missing,
                Err(e) => return Err(e),
            };
            if let FileBefore::Bytes(bytes) = &before {
                // An earlier append in any scope must be upgraded too: merely
                // restoring its length after this overwrite would corrupt it.
                if let Some(undo) = j.files.get_mut(&path) {
                    undo.before.preserve_bytes(bytes)?;
                }
                for wj in &mut j.workers {
                    if let Some(undo) = wj.files.get_mut(&path) {
                        undo.before.preserve_bytes(bytes)?;
                    }
                }
            }
            let order = j.file_order;
            j.file_order += 1;
            j.files.entry(path.clone()).or_insert_with(|| FileUndo { order, before: before.clone() });
            if let Some(w) = worker {
                j.worker(w).files.entry(path.clone()).or_insert(FileUndo { order, before });
            }
            Ok(())
        };
        preserve().map_err(|e| EvalError::new(format!("cannot preserve file {} before writing: {e}", path.display())))?;
        // Keep managed writes under the same lock as their snapshots: the
        // undo order must be the actual write order, even across workers.
        let result = write();
        drop(j);
        result
    }

    /// Worker `w` starts a level, with registers `regs`.
    pub(crate) fn mark(&self, w: usize, regs: Registers) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        let mut j = self.journal.lock().unwrap();
        *j.worker(w) = WorkerJournal { mark: Some(regs), ..Default::default() };
    }

    /// Worker `w` ends, with registers `regs`.
    pub(crate) fn worker_end(&self, w: usize, regs: Registers) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        self.journal.lock().unwrap().worker(w).end = Some(regs);
    }

    /// A lazy evaluation may have produced effects: undo its cached result
    /// along with the same run/worker-level journal, so it can replay them.
    pub(crate) fn memo_undo(&self, worker: Option<usize>, undo: MemoUndo) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        let mut j = self.journal.lock().unwrap();
        if let Some(w) = worker {
            let wj = j.worker(w);
            if wj.mark.is_some() {
                wj.memos.push(undo.clone());
            }
        }
        j.memos.push(undo);
    }

    /// The search resumes at the start of the level it was expanding:
    /// undo what the workers in `rolled_back` did in that level.
    pub(crate) fn rollback_level(&self, rolled_back: &[bool]) -> std::io::Result<()> {
        let mut j = self.journal.lock().unwrap();
        let workers = std::mem::take(&mut j.workers);
        let mut files: HashMap<PathBuf, FileUndo> = HashMap::new();
        let mut resume = Vec::new();
        let mut memos = Vec::new();
        for (w, wj) in workers.into_iter().enumerate() {
            let back = rolled_back.get(w).copied().unwrap_or(false);
            if back {
                memos.extend(wj.memos);
                for h in wj.lines {
                    *j.owed.entry(h).or_default() += 1;
                }
                // (Before the results still owed: the worker got those
                // after these, or did not get to them.)
                for (h, v) in wj.runs.into_iter().rev() {
                    j.owed_runs.entry(h).or_default().push_front(v);
                }
                for (p, undo) in wj.files {
                    let e = files.entry(p).or_insert_with(|| FileUndo { order: undo.order, before: undo.before.clone() });
                    if undo.order < e.order {
                        *e = undo;
                    }
                }
            }
            resume.push(if back { wj.mark.or(wj.end) } else { wj.end });
        }
        for (p, undo) in files {
            undo.before.restore(&p)?;
        }
        j.resume = resume;
        self.owing.store(!j.owed.is_empty(), Ordering::Relaxed);
        self.owing_runs.store(!j.owed_runs.is_empty(), Ordering::Relaxed);
        drop(j);
        for undo in memos {
            undo();
        }
        Ok(())
    }

    /// The run starts over: undo everything it did.
    pub(crate) fn rollback_run(&self) -> std::io::Result<()> {
        let mut j = self.journal.lock().unwrap();
        j.owed = j.printed.clone();
        j.owed_runs = j.ran.iter().map(|(h, vs)| (h.clone(), vs.iter().cloned().collect())).collect();
        for (p, undo) in &j.files {
            undo.before.restore(p)?;
        }
        let memos = std::mem::take(&mut j.memos);
        j.workers.clear();
        j.resume.clear();
        self.owing.store(!j.owed.is_empty(), Ordering::Relaxed);
        self.owing_runs.store(!j.owed_runs.is_empty(), Ordering::Relaxed);
        drop(j);
        for undo in memos {
            undo();
        }
        Ok(())
    }

    /// The compiled tier's pre-run is over: it is rolled back as a run
    /// started over (`rollback_run`), and the results of the commands it ran
    /// stay owed to the search (or simulation) that follows; the rest of the
    /// journal is forgotten.
    pub(crate) fn finish_prerun(&self) -> std::io::Result<()> {
        self.rollback_run()?;
        self.active.store(false, Ordering::SeqCst);
        let mut j = self.journal.lock().unwrap();
        // (Also as run: a restart of the search owes them again, before
        // what it ran itself.)
        let owed_runs = std::mem::take(&mut j.owed_runs);
        let ran = owed_runs.iter().map(|(h, vs)| (h.clone(), vs.iter().cloned().collect())).collect();
        *j = Journal { ran, owed_runs, ..Journal::default() };
        self.owing.store(false, Ordering::SeqCst);
        self.owing_runs.store(!j.owed_runs.is_empty(), Ordering::SeqCst);
        Ok(())
    }

    /// The registers each worker of the next search starts from (a search
    /// recovered from a checkpoint).
    pub(crate) fn set_resume(&self, resume: Vec<Option<Registers>>) {
        self.journal.lock().unwrap().resume = resume;
    }

    /// The registers worker `w` of a resumed search starts from (once).
    pub(crate) fn take_resume_registers(&self, w: usize) -> Option<Registers> {
        let mut j = self.journal.lock().unwrap();
        j.resume.get_mut(w).and_then(Option::take)
    }

    /// The run is over: stop recording and forget the journal.
    pub(crate) fn finish(&self) {
        self.active.store(false, Ordering::SeqCst);
        *self.journal.lock().unwrap() = Journal::default();
        self.owing.store(false, Ordering::SeqCst);
        self.owing_runs.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::Hasher;
    use std::io::Write;

    #[derive(Default)]
    pub(super) struct ConstantHasher;

    impl Hasher for ConstantHasher {
        fn finish(&self) -> u64 {
            0
        }
        fn write(&mut self, _: &[u8]) {}
    }

    #[test]
    fn colliding_lines_do_not_consume_each_others_replay() {
        let e = Effects::default();
        e.set_active(true);
        e.print("first", None);
        e.print("first", None);
        e.rollback_run().unwrap();
        e.print("different", None);
        assert_eq!(owed(&e), 2, "a distinct line must not consume an owed line");
        e.print("first", None);
        e.print("first", None);
        assert_eq!(owed(&e), 0);
    }

    #[test]
    fn colliding_commands_do_not_consume_each_others_results() {
        let e = Effects::default();
        e.set_active(true);
        let first = 1;
        let different = 2;
        let mut runs = 0;
        assert_eq!(run(&e, first, None, &mut runs), Value::Int(1));
        e.rollback_run().unwrap();
        assert_eq!(run(&e, different, None, &mut runs), Value::Int(2));
        assert_eq!(run(&e, first, None, &mut runs), Value::Int(1));
    }

    fn argv(args: &[&str]) -> Value {
        Value::tuple(args.iter().map(|s| Value::str(s)).collect())
    }

    fn command(n: u64) -> PreparedCommand {
        PreparedCommand::io_exec(&argv(&[&n.to_string()])).unwrap()
    }

    #[cfg(unix)]
    fn env_command(v: Value) -> PreparedCommand {
        let env = Value::record(vec![(tlx_value::Sym::new("X"), v)]);
        PreparedCommand::io_env_exec(&env, &argv(&["/bin/sh", "-c", "printf %s \"$X\""])).unwrap()
    }

    #[test]
    fn prepared_keys_preserve_operator_and_argument_boundaries() {
        let args = argv(&["program", "a", "bc"]);
        let empty = argv(&[]);
        let keys = [
            PreparedCommand::io_exec(&args).unwrap(),
            PreparedCommand::io_env_exec(&empty, &args).unwrap(),
            PreparedCommand::io_exec_template(&args, &empty).unwrap(),
            PreparedCommand::io_env_exec_template(&empty, &args, &empty).unwrap(),
        ];
        let mut map = ReplayMap::default();
        for (i, key) in keys.into_iter().enumerate() {
            map.insert(key, i);
        }
        assert_eq!(map.len(), 4, "operator identity survives constant hashing");
        assert_ne!(PreparedCommand::io_exec(&args).unwrap(), PreparedCommand::io_exec(&argv(&["program", "ab", "c"])).unwrap());
        assert_ne!(PreparedCommand::io_exec(&argv(&["program"])).unwrap(), PreparedCommand::io_exec(&argv(&["program", ""])).unwrap());
        // Equal argument representations are interchangeable only after the
        // native's sequence coercion has resolved their operational behavior.
        let f = Value::fcn_rcd_from_pairs(vec![(Value::Int(1), Value::str("program"))]).unwrap();
        assert_eq!(PreparedCommand::io_exec(&f).unwrap(), PreparedCommand::io_exec(&argv(&["program"])).unwrap());
        assert!(PreparedCommand::io_exec(&Value::Int(1)).is_err());
        assert!(PreparedCommand::io_exec_template(&argv(&["%d"]), &argv(&["x"])).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn equal_environment_values_with_different_printing_run_separately() {
        let e = Effects::default();
        e.set_active(true);
        let intv = Value::Interval(1, 2);
        let set = Value::set_from(vec![Value::Int(1), Value::Int(2)]).unwrap();
        assert_eq!(intv, set);
        let a = env_command(intv);
        let b = env_command(set);
        let first = e.run_command(&a, None, || a.run()).unwrap();
        e.rollback_run().unwrap();
        let different = e.run_command(&b, None, || b.run()).unwrap();
        assert_ne!(first, different);
        assert_eq!(e.journal.lock().unwrap().ran.len(), 2);
        let replayed = e.run_command(&a, None, || panic!("must replay the interval environment")).unwrap();
        assert_eq!(first, replayed);
    }

    #[test]
    #[cfg(unix)]
    fn opaque_membership_inside_lazy_sets_preserves_the_full_print_observation() {
        use tlx_value::lazy::{CustomSet, LazySet};
        struct Filter(i64);
        impl std::fmt::Debug for Filter {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("Filter")
            }
        }
        impl CustomSet for Filter {
            fn enumerate(&self) -> tlx_value::Result<Option<Vec<Value>>> {
                Ok(None)
            }
            fn contains(&self, v: &Value) -> tlx_value::Result<bool> {
                Ok(matches!(v, Value::Tuple(es) if es[0] == Value::Int(self.0)))
            }
        }
        let product = Value::LazySet(Arc::new(LazySet::Product(vec![
            Value::set_from(vec![Value::Int(1), Value::Int(2)]).unwrap(),
            Value::set_from(vec![Value::Int(0)]).unwrap(),
        ])));
        let filtered =
            |n| Value::LazySet(Arc::new(LazySet::Cap(product.clone(), Value::LazySet(Arc::new(LazySet::Custom(Box::new(Filter(n))))))));
        let a = filtered(1);
        let b = filtered(2);
        assert_ne!(a.to_string(), b.to_string());
        let a = env_command(a);
        let b = env_command(b);
        assert_ne!(a, b);
        let e = Effects::default();
        e.set_active(true);
        let first = e.run_command(&a, None, || a.run()).unwrap();
        e.rollback_run().unwrap();
        let different = e.run_command(&b, None, || b.run()).unwrap();
        assert_ne!(first, different);
    }

    #[test]
    fn failed_distinct_command_does_not_consume_or_record_replay() {
        let e = Effects::default();
        e.set_active(true);
        let mut runs = 0;
        run(&e, 1, None, &mut runs);
        e.rollback_run().unwrap();
        assert!(e.run_command(&command(2), None, || Err(EvalError::new("bad command"))).is_err());
        assert_eq!(run(&e, 1, None, &mut runs), Value::Int(1));
        assert_eq!(runs, 1);
        assert_eq!(e.journal.lock().unwrap().ran.len(), 1);
    }

    #[test]
    #[cfg(unix)]
    fn lazy_command_forcing_keeps_the_native_evaluation_context() {
        #[derive(Debug, Default)]
        struct Command(std::sync::OnceLock<Value>);
        impl tlx_value::lazy::LazyFcn for Command {
            fn domain(&self) -> tlx_value::Result<Value> {
                Ok(Value::Interval(1, 2))
            }
            fn apply(&self, arg: &Value) -> tlx_value::Result<Value> {
                if *arg == Value::Int(1) {
                    return Ok(Value::str("/bin/echo"));
                }
                Ok(self
                    .0
                    .get_or_init(|| {
                        let union = Value::LazySet(Arc::new(tlx_value::lazy::LazySet::Cup(Value::Interval(1, 1), Value::Interval(2, 2))));
                        let env = Value::record(vec![(tlx_value::Sym::new("X"), union)]);
                        let cmd = Value::tuple(vec![Value::str("/bin/sh"), Value::str("-c"), Value::str("printf %s \"$X\"")]);
                        let out = crate::natives::io_env_exec(&env, &cmd).unwrap();
                        let Value::Record(fs) = out else { unreachable!() };
                        fs.iter().find(|(k, _)| k.as_str() == "stdout").unwrap().1.clone()
                    })
                    .clone())
            }
        }
        let run = |active| {
            let e = Effects::default();
            e.set_active(active);
            let cmd = Value::LazyFcn(Arc::new(Command::default()));
            let prepared = PreparedCommand::io_exec(&cmd).unwrap();
            e.run_command(&prepared, None, || prepared.run()).unwrap()
        };
        assert_eq!(run(false), run(true));
    }

    #[test]
    #[cfg(unix)]
    fn a_lazy_command_argument_is_forced_only_once() {
        #[derive(Debug)]
        struct Command(Arc<std::sync::atomic::AtomicUsize>);
        impl tlx_value::lazy::LazyFcn for Command {
            fn domain(&self) -> tlx_value::Result<Value> {
                Ok(Value::Interval(1, 2))
            }
            fn apply(&self, arg: &Value) -> tlx_value::Result<Value> {
                self.0.fetch_add(1, Ordering::Relaxed);
                Ok(Value::str(if *arg == Value::Int(1) { "/bin/echo" } else { "argument" }))
            }
        }
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cmd = Value::LazyFcn(Arc::new(Command(calls.clone())));
        let e = Effects::default();
        e.set_active(true);
        let prepared = PreparedCommand::io_exec(&cmd).unwrap();
        e.run_command(&prepared, None, || prepared.run()).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2, "each of the two arguments is evaluated once");
    }

    #[test]
    fn recreated_lazy_commands_replay_and_snapshot_nested_effects_outside_lock() {
        struct Command(Arc<Effects>);
        impl std::fmt::Debug for Command {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("Command")
            }
        }
        impl tlx_value::lazy::LazyFcn for Command {
            fn domain(&self) -> tlx_value::Result<Value> {
                Ok(Value::Interval(1, 1))
            }
            fn apply(&self, _: &Value) -> tlx_value::Result<Value> {
                self.0.print("nested", None);
                Ok(Value::str("program"))
            }
        }
        let e = Arc::new(Effects::default());
        e.set_active(true);
        let command = || Value::LazyFcn(Arc::new(Command(e.clone())));
        let mut runs = 0;
        let mut eval = || {
            let prepared = PreparedCommand::io_exec(&command()).unwrap();
            e.run_command(&prepared, None, || {
                runs += 1;
                Ok(Value::Int(runs))
            })
            .unwrap()
        };
        assert_eq!(eval(), Value::Int(1));
        e.rollback_run().unwrap();
        assert_eq!(eval(), Value::Int(1));
        assert_eq!(runs, 1);
    }

    fn write(e: &Effects, path: &std::path::Path, w: usize, kind: FileWrite, bytes: &[u8]) {
        e.file_write(path.to_str().unwrap(), Some(w), kind, || {
            match kind {
                FileWrite::Append => std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap().write_all(bytes).unwrap(),
                FileWrite::Overwrite => std::fs::write(path, bytes).unwrap(),
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn mixed_file_writes_restore_level_and_run_bytes() {
        let dir = tlx_testkit::TempDir::new("effects_files");
        let path = dir.join("out");
        for initial in [None, Some(&b"original"[..])] {
            let _ = std::fs::remove_file(&path);
            if let Some(bytes) = initial {
                std::fs::write(&path, bytes).unwrap();
            }
            let e = Effects::default();
            e.set_active(true);
            e.mark(0, regs(0));
            write(&e, &path, 0, FileWrite::Append, b" first");
            let level_start = std::fs::read(&path).unwrap();
            e.mark(0, regs(1));
            write(&e, &path, 0, FileWrite::Append, b" second");
            write(&e, &path, 0, FileWrite::Overwrite, b"new");
            write(&e, &path, 0, FileWrite::Append, b" tail");
            write(&e, &path, 0, FileWrite::Overwrite, b"equal length");
            e.rollback_level(&[true]).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), level_start);
            // A repeated rollback and then a full restart must still use
            // the run's original checkpoint, not the last level's bytes.
            e.mark(0, regs(1));
            write(&e, &path, 0, FileWrite::Overwrite, b"replacement");
            e.rollback_level(&[true]).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), level_start);
            e.rollback_run().unwrap();
            match initial {
                Some(bytes) => assert_eq!(std::fs::read(&path).unwrap(), bytes),
                None => assert!(!path.exists()),
            }
        }
    }

    #[test]
    fn shared_file_rollback_uses_first_write_order_and_upgrades_appends() {
        let dir = tlx_testkit::TempDir::new("effects_workers");
        let path = dir.join("out");
        for first in [FileWrite::Append, FileWrite::Overwrite] {
            std::fs::write(&path, b"original").unwrap();
            let e = Effects::default();
            e.set_active(true);
            e.mark(0, regs(0));
            e.mark(1, regs(0));
            // Worker 1 goes first, but worker 0's entry is visited first
            // during rollback. The shortest length is not the earliest.
            write(&e, &path, 1, first, b"longer replacement");
            write(&e, &path, 0, FileWrite::Overwrite, b"x");
            e.rollback_level(&[true, true]).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"original");
        }
    }

    #[test]
    fn file_preservation_and_restoration_failures_are_reported() {
        let dir = tlx_testkit::TempDir::new("effects_failure");
        let e = Effects::default();
        e.set_active(true);
        // A directory cannot be backed up as a regular file.
        assert!(e.file_write(dir.to_str().unwrap(), None, FileWrite::Overwrite, || Ok(())).is_err());
        assert!(e.journal.lock().unwrap().files.is_empty());
        let path = dir.join("out");
        std::fs::write(&path, b"original").unwrap();
        write(&e, &path, 0, FileWrite::Overwrite, b"new");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(e.rollback_run().unwrap_err().to_string().contains("cannot restore file"));
    }

    fn regs(v: i64) -> Registers {
        Registers::from_iter([(1, Value::Int(v))])
    }

    fn owed(e: &Effects) -> u64 {
        e.journal.lock().unwrap().owed.values().sum()
    }

    #[test]
    fn rolled_back_lines_are_owed_once() {
        let e = Effects::default();
        e.set_active(true);
        e.mark(0, regs(1));
        e.print("a", Some(0));
        e.print("b", Some(0));
        e.worker_end(0, regs(2));
        e.rollback_level(&[true]).unwrap();
        assert_eq!(e.take_resume_registers(0), Some(regs(1)));
        assert_eq!(owed(&e), 2);
        // The resumed level evaluates "a" and "b" again (owed: not printed),
        // then "a" once more (printed).
        e.mark(0, regs(1));
        e.print("a", Some(0));
        e.print("b", Some(0));
        assert!(!e.owing.load(Ordering::Relaxed));
        e.print("a", Some(0));
        assert_eq!(e.journal.lock().unwrap().printed["a"], 2);
    }

    #[test]
    fn workers_outside_the_level_keep_their_registers() {
        let e = Effects::default();
        e.set_active(true);
        e.mark(0, regs(1));
        e.mark(1, regs(5));
        e.print("x", Some(1));
        e.worker_end(0, regs(3));
        e.worker_end(1, regs(6));
        e.rollback_level(&[false, true]).unwrap();
        assert_eq!(e.take_resume_registers(0), Some(regs(3)));
        assert_eq!(e.take_resume_registers(1), Some(regs(5)));
        assert_eq!(e.take_resume_registers(1), None);
        assert_eq!(owed(&e), 1);
    }

    fn run(e: &Effects, cmd: u64, w: Option<usize>, counter: &mut i64) -> Value {
        e.run_command(&command(cmd), w, || {
            *counter += 1;
            Ok(Value::Int(*counter))
        })
        .unwrap()
    }

    #[test]
    fn rolled_back_commands_are_not_run_again() {
        let e = Effects::default();
        let mut runs = 0;
        e.set_active(true);
        e.mark(0, regs(0));
        assert_eq!(run(&e, 7, Some(0), &mut runs), Value::Int(1));
        assert_eq!(run(&e, 7, Some(0), &mut runs), Value::Int(2));
        assert_eq!(run(&e, 8, Some(0), &mut runs), Value::Int(3));
        e.rollback_level(&[true]).unwrap();
        // The resumed level gets the same results in the same order, then
        // runs the command again.
        e.mark(0, regs(0));
        assert_eq!(run(&e, 7, Some(0), &mut runs), Value::Int(1));
        // Rolled back again in the middle: owed again, before the rest.
        e.rollback_level(&[true]).unwrap();
        e.mark(0, regs(0));
        assert_eq!(run(&e, 7, Some(0), &mut runs), Value::Int(1));
        assert_eq!(run(&e, 7, Some(0), &mut runs), Value::Int(2));
        assert_eq!(run(&e, 7, Some(0), &mut runs), Value::Int(4));
        assert_eq!(run(&e, 8, Some(0), &mut runs), Value::Int(3));
        // Starting over: every result since the start is owed.
        e.rollback_run().unwrap();
        for want in [1, 2, 4] {
            assert_eq!(run(&e, 7, None, &mut runs), Value::Int(want));
        }
        assert_eq!(runs, 4);
        e.finish();
        assert_eq!(run(&e, 7, None, &mut runs), Value::Int(5));
    }

    #[test]
    fn prerun_results_are_owed_to_the_search() {
        let e = Effects::default();
        let mut runs = 0;
        e.set_active(true);
        e.print("p", None);
        assert_eq!(run(&e, 1, None, &mut runs), Value::Int(1));
        e.finish_prerun().unwrap();
        assert_eq!(owed(&e), 0);
        // The search (no restart possible) gets the pre-run's result once.
        assert_eq!(run(&e, 1, Some(0), &mut runs), Value::Int(1));
        assert_eq!(run(&e, 1, Some(0), &mut runs), Value::Int(2));
        assert!(!e.owing_runs.load(Ordering::Relaxed));
    }

    #[test]
    fn a_restart_after_the_prerun_owes_its_results_first() {
        let e = Effects::default();
        let mut runs = 0;
        e.set_active(true);
        assert_eq!(run(&e, 1, None, &mut runs), Value::Int(1));
        e.finish_prerun().unwrap();
        e.set_active(true);
        assert_eq!(run(&e, 1, Some(0), &mut runs), Value::Int(1));
        assert_eq!(run(&e, 1, Some(0), &mut runs), Value::Int(2));
        e.rollback_run().unwrap();
        for want in [1, 2, 3] {
            assert_eq!(run(&e, 1, Some(0), &mut runs), Value::Int(want));
        }
        assert_eq!(runs, 3);
    }

    #[test]
    fn starting_over_owes_every_line() {
        let e = Effects::default();
        e.set_active(true);
        e.print("m", None);
        e.mark(0, regs(0));
        e.print("w", Some(0));
        e.rollback_run().unwrap();
        assert_eq!(owed(&e), 2);
        assert!(e.journal.lock().unwrap().resume.is_empty());
        e.finish();
        assert_eq!(owed(&e), 0);
    }
}
