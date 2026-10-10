//! The state of a run of an `Interp` (as opposed to the program and its
//! analysis): what `TLCGet` reports about the run, the `TLCSet` registers,
//! the exit request, the side effects across restarts, and the statistics
//! of the last search, together with the `Interp` methods that read and
//! maintain them. (The generators of Randomization and the counts of
//! composed and rejected states, which the enumeration updates, are still
//! fields of `Interp`; the thread-local register and output captures those
//! methods use are declared in the parent module.)

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::ThreadId;

use tlx_value::Value;

use crate::error::EvalError;

use super::effects::Registers;
use super::{
    effects, read_registers, write_registers, HoldMark, Interp, RunStats, SavedRegisters, SimConfig, SpecInfo, DRY, HELD, HOLDING,
    REGISTERS,
};

/// Temporary observational evaluation, including the shared register copies
/// this thread may update. No register lock is held while evaluating.
pub(super) struct DryEval<'a> {
    interp: &'a Interp,
    registers: Registers,
    worker_copies: Vec<(usize, Registers)>,
    was_dry: bool,
}

impl<'a> DryEval<'a> {
    pub(super) fn new(interp: &'a Interp) -> Self {
        let me = std::thread::current().id();
        let registers = interp.with_registers(|regs, _| regs.clone());
        let worker_copies = interp.copies_of_registers(me == interp.run.main_thread, me);
        Self { interp, registers, worker_copies, was_dry: DRY.replace(true) }
    }
}

impl Drop for DryEval<'_> {
    fn drop(&mut self) {
        DRY.set(self.was_dry);
        self.interp.with_registers(|regs, _| *regs = std::mem::take(&mut self.registers));
        let mut all = self.interp.run.worker_regs.lock().unwrap();
        for (w, regs) in self.worker_copies.drain(..) {
            if let Some(Some((_, r))) = all.get_mut(w) {
                *r = regs;
            }
        }
    }
}

pub(super) struct RunState {
    /// `TLCSet("exit", TRUE)` was evaluated.
    pub(super) exit: AtomicBool,
    /// Print/PrintT evaluate to their TLC values but do not print (the
    /// compiled tier's pre-run; the search prints the same lines).
    pub(super) suppress_print: AtomicBool,
    /// Run configuration reported by `TLCGet("config")`.
    pub(super) workers: AtomicUsize,
    pub(super) check_deadlock: AtomicBool,
    /// The simulation (`-simulate` / `-generate`), if the run is one rather
    /// than a breadth-first search.
    pub(super) simulation: OnceLock<SimConfig>,
    /// Start of the simulation (`TLCGet("duration")` during it).
    pub(super) sim_start: OnceLock<std::time::Instant>,
    /// TLCGet/TLCSet registers as set from the main thread (ASSUME, Init);
    /// each worker thread starts from a copy (TLC: `setAllValues`).
    pub(super) main_registers: Mutex<Registers>,
    pub(super) main_thread: ThreadId,
    /// Per worker of the last search or simulation that used the registers
    /// (by `tlx_runtime::live::current_worker`), its thread and a copy of
    /// its registers: what the main thread reads once workers have run
    /// (TLC: `getValue(0, i)`, e.g. in POSTCONDITION), and what
    /// `TLCGet("all")` reports for every worker. Cleared when the workers
    /// of a run are replaced (`new_workers`).
    pub(super) worker_regs: Mutex<Vec<Option<(ThreadId, Registers)>>>,
    /// Side effects across the restarts of a compiled-tier run.
    pub(super) effects: effects::Effects,
    /// `TLCGet("stats")`: after model checking (for POSTCONDITION), or
    /// during a simulation.
    pub(super) final_stats: Mutex<Option<Value>>,
    /// `TLCGet("stats")` in a simulation (TLC's record differs).
    pub(super) final_sim_stats: Mutex<Option<Value>>,
    /// `TLCGet("spec")` and the records of `TLCGet("action")`.
    pub(super) spec_info: OnceLock<SpecInfo>,
}

impl RunState {
    pub(super) fn new() -> RunState {
        RunState {
            exit: AtomicBool::new(false),
            suppress_print: AtomicBool::new(false),
            workers: AtomicUsize::new(1),
            check_deadlock: AtomicBool::new(true),
            simulation: OnceLock::new(),
            sim_start: OnceLock::new(),
            main_registers: Mutex::new(Registers::default()),
            main_thread: std::thread::current().id(),
            worker_regs: Mutex::new(Vec::new()),
            effects: Default::default(),
            final_stats: Mutex::new(None),
            final_sim_stats: Mutex::new(None),
            spec_info: OnceLock::new(),
        }
    }
}

impl Interp {
    // --- run configuration and TLCGet/TLCSet state ---

    /// The interpreter's unique id (key of per-thread caches).
    pub fn uid(&self) -> u64 {
        self.uid
    }

    /// Record the run configuration (for `TLCGet("config")`).
    pub fn set_run_config(&self, workers: usize, check_deadlock: bool) {
        self.run.workers.store(workers, Ordering::Relaxed);
        self.run.check_deadlock.store(check_deadlock, Ordering::Relaxed);
    }

    /// Record the statistics of the finished run (`TLCGet("stats")`).
    pub fn set_final_stats(&self, s: RunStats) {
        *self.run.final_stats.lock().unwrap() = Some(Self::stats_record(&s));
        *self.run.final_sim_stats.lock().unwrap() = s.traces.is_some().then(|| Self::sim_stats_record(&s));
    }

    pub(crate) fn final_sim_stats(&self) -> Option<Value> {
        self.run.final_sim_stats.lock().unwrap().clone()
    }

    /// The `TLCGet("stats")` record of `s`.
    pub(crate) fn stats_record(s: &RunStats) -> Value {
        let mut fs = vec![
            ("distinct", s.distinct),
            ("generated", s.generated),
            ("queue", s.queue),
            ("diameter", s.diameter),
            ("duration", s.duration_secs),
            ("worker", 0),
            ("initial", s.initial),
        ];
        if let Some(t) = s.traces {
            fs.push(("traces", t));
        }
        Value::record(fs.iter().map(|(n, v)| (tlx_value::Sym::new(n), Value::Int(*v as i64))).collect())
    }

    /// `TLCGet("stats")` in a simulation, as TLC's `Simulator.getStatistics`
    /// (with `-Dtlc2.tool.Simulator.extendedStatistics` off: no distinct
    /// states, values or retries). TLC's `behavior` record counts the
    /// actions of the current behavior; here, as after the simulation and
    /// in the initial states, it is empty.
    pub(crate) fn sim_stats_record(s: &RunStats) -> Value {
        let int = |v: i64| Value::Int(v);
        let traces = s.traces.unwrap_or(0) as i64;
        let empty = Value::record(Vec::new());
        let sym = tlx_value::Sym::new;
        Value::record(vec![
            (sym("traces"), int(traces)),
            (sym("duration"), int(s.duration_secs as i64)),
            (sym("generated"), int(s.generated as i64)),
            (sym("behavior"), Value::record(vec![(sym("id"), int(traces)), (sym("actions"), empty.clone())])),
            (sym("worker"), int(0)),
            (sym("distinct"), int(-1)),
            (sym("distinctvalues"), int(-1)),
            (sym("retries"), int(-1)),
            (sym("actions"), empty),
            (sym("levelmean"), int(s.level_mean as i64)),
            (sym("levelvariance"), int(s.level_variance)),
        ])
    }

    pub(crate) fn final_stats(&self) -> Option<Value> {
        self.run.final_stats.lock().unwrap().clone()
    }

    /// Record what `TLCGet("spec")` and `TLCGet("action")` report.
    pub(crate) fn set_spec_info(&self, info: SpecInfo) {
        let _ = self.run.spec_info.set(info);
    }

    /// Whether the spec applies `TLCExt!Trace` (the search then keeps the
    /// paths to the states, `tlx_runtime::Model::uses_trace`).
    pub fn uses_trace(&self) -> bool {
        self.uses_trace
    }

    pub(crate) fn spec_info(&self) -> Option<&SpecInfo> {
        self.run.spec_info.get()
    }

    /// Record that the run is a simulation (`TLCGet("config")`).
    pub fn set_simulation(&self, config: SimConfig) {
        let _ = self.run.simulation.set(config);
        self.run.sim_start.get_or_init(std::time::Instant::now);
        self.set_final_stats(RunStats { traces: Some(0), ..Default::default() });
    }

    /// Seconds since the simulation started.
    pub(crate) fn sim_duration(&self) -> Option<u64> {
        self.run.sim_start.get().map(|t| t.elapsed().as_secs())
    }

    pub(crate) fn is_simulation(&self) -> bool {
        self.run.simulation.get().is_some()
    }

    pub(crate) fn sim_config(&self) -> Option<&SimConfig> {
        self.run.simulation.get()
    }

    pub(crate) fn run_config(&self) -> (usize, bool) {
        (self.run.workers.load(Ordering::Relaxed), self.run.check_deadlock.load(Ordering::Relaxed))
    }

    pub fn exit_requested(&self) -> bool {
        self.run.exit.load(Ordering::Relaxed)
    }

    /// Whether Print/PrintT output is suppressed (see `set_suppress_print`).
    pub fn print_suppressed(&self) -> bool {
        self.run.suppress_print.load(Ordering::Relaxed)
    }

    /// Set whether Print/PrintT evaluate without printing their output:
    /// the compiled tier's shape pre-run evaluates the spec, but TLC prints
    /// those lines only in the search.
    pub fn set_suppress_print(&self, on: bool) {
        self.run.suppress_print.store(on, Ordering::Relaxed);
    }

    pub(crate) fn request_exit(&self) {
        self.run.exit.store(true, Ordering::Relaxed);
    }

    /// Undo an exit requested by `TLCSet("exit", TRUE)`: a search that
    /// starts again requests it again where TLC does.
    pub fn undo_exit(&self) {
        self.run.exit.store(false, Ordering::Relaxed);
    }

    /// Run `f` on the registers of the current thread: the shared main-thread
    /// registers, or this worker's copy of them (made on first use); also
    /// the index of this thread among the workers of the search, if it is
    /// one.
    pub(super) fn with_registers<R>(&self, f: impl FnOnce(&mut Registers, Option<usize>) -> R) -> R {
        if std::thread::current().id() == self.run.main_thread {
            return f(&mut self.run.main_registers.lock().unwrap(), None);
        }
        let key = self.uid;
        REGISTERS.with(|r| {
            let mut r = r.borrow_mut();
            let (regs, worker) = r.entry(key).or_insert_with(|| {
                let worker = tlx_runtime::live::current_worker();
                // (A worker of a resumed search continues with its
                // registers of the interrupted one.)
                let resumed = worker.and_then(|w| self.run.effects.take_resume_registers(w));
                let regs = resumed.unwrap_or_else(|| self.run.main_registers.lock().unwrap().clone());
                // A worker of a new search (or simulation) replaces the one
                // of an earlier search, e.g. a compiled tier's pre-run.
                if let Some(w) = worker {
                    let mut all = self.run.worker_regs.lock().unwrap();
                    if all.len() <= w {
                        all.resize_with(w + 1, || None);
                    }
                    all[w] = Some((std::thread::current().id(), regs.clone()));
                }
                (regs, worker)
            });
            f(regs, *worker)
        })
    }

    /// The workers of the run are replaced (e.g. a restarted or resumed
    /// search): the next workers to use the registers start from the main
    /// thread's registers.
    pub fn new_workers(&self) {
        self.run.worker_regs.lock().unwrap().clear();
    }

    /// The registers of every worker of the last search (TLC's
    /// `getAllValues`/`getAllNamedRegisterValues`), for `TLCGet("all")`
    /// (`named` false: the integer registers) and `TLCGet("all:named")`:
    /// per key of worker 0's registers (as TLC), the value of each worker
    /// (a worker that did not use the registers has the main thread's).
    pub(crate) fn all_registers(&self, named: bool) -> Vec<(i64, Vec<Option<Value>>)> {
        let workers = self.run.workers.load(Ordering::Relaxed).max(1);
        let all = self.run.worker_regs.lock().unwrap();
        let main = self.run.main_registers.lock().unwrap();
        let regs = |w: usize| all.get(w).and_then(Option::as_ref).map_or(&*main, |(_, r)| r);
        let mut keys: Vec<i64> = regs(0).keys().copied().filter(|&k| (k < 0) == named).collect();
        keys.sort_unstable();
        keys.into_iter().map(|k| (k, (0..workers).map(|w| regs(w).get(&k).cloned()).collect())).collect()
    }

    /// Print `line` (Print/PrintT output), unless it is owed by a restarted
    /// run (see `effects`) or evaluated without effects.
    pub(crate) fn print_line(&self, line: &str) {
        // (Held back also without effects: TLX_PARANOID compares the lines
        // of a compiled run with the interpreter's.)
        if HOLDING.get() {
            HELD.with(|h| h.borrow_mut().push(line.to_string()));
            return;
        }
        if DRY.get() {
            return;
        }
        self.run.effects.print(line, tlx_runtime::live::current_worker());
    }

    /// Run `f` with the Print/PrintT output of this thread held back
    /// instead of printed; returns `f`'s result and the lines. A compiled
    /// run that may fail (and then be redone by the interpreter) prints
    /// only once it succeeds (`release_output`), so a line is not printed
    /// twice. Unwinding discards this scope's lines and restores the enclosing capture.
    pub fn hold_output<R>(f: impl FnOnce() -> R) -> (R, Vec<String>) {
        let mark = Interp::hold_begin();
        let r = f();
        (r, Interp::hold_end(mark))
    }

    /// `hold_output` in two steps: hold this thread's output back from now
    /// on (until `hold_end` with the mark returned). Dropping the mark instead
    /// discards this scope's lines and restores the enclosing capture.
    #[inline]
    pub fn hold_begin() -> HoldMark {
        HoldMark { was: HOLDING.replace(true), start: HELD.with(|h| h.borrow().len()) }
    }

    /// The lines held back since `hold_begin` returned `mark`.
    #[inline]
    pub fn hold_end(mark: HoldMark) -> Vec<String> {
        HELD.with(|h| {
            let mut h = h.borrow_mut();
            if h.len() == mark.start {
                Vec::new()
            } else {
                h.split_off(mark.start)
            }
        })
    }

    /// Print the lines `hold_output` held back (as Print/PrintT would have).
    pub fn release_output(&self, lines: Vec<String>) {
        for l in lines {
            self.print_line(&l);
        }
    }

    /// Write already-serialized bytes under the effect journal. Preparing
    /// a native value can evaluate nested effects and must precede this call.
    pub(crate) fn with_file_write(&self, path: &str, kind: effects::FileWrite, bytes: &[u8], op: &str) -> crate::error::EResult<()> {
        if DRY.get() {
            return Ok(());
        }
        self.run.effects.file_write(path, tlx_runtime::live::current_worker(), kind, || {
            let file = std::path::Path::new(path);
            let result = match kind {
                effects::FileWrite::Append => tlx_runtime::io::append_to(file, bytes),
                effects::FileWrite::Overwrite => tlx_runtime::io::write_creating_parents(file, bytes),
            };
            result.map_err(|e| EvalError::new(format!("{op}: cannot write {path}: {e}")))
        })
    }

    /// Intermediate states of outer action compositions count as generated.
    /// Paranoid re-evaluation is observational and must not count them twice.
    pub fn count_composed_states(&self, count: u64) {
        if count != 0 && !DRY.get() {
            self.composed_states.fetch_add(count, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Run `f` (evaluations on this thread) without lasting side effects:
    /// Print/PrintT print nothing, managed file writers write nothing, and the
    /// registers of this thread are restored afterwards (TLX_PARANOID's
    /// interpreter evaluations, which repeat the compiled ones). IOExec
    /// still runs (neither recorded nor owed). Restoration also runs during unwinding.
    pub fn without_effects<R>(&self, f: impl FnOnce() -> R) -> R {
        let _scope = DryEval::new(self);
        f()
    }

    // --- side effects across the restarts of a compiled-tier run ---

    /// Whether `effects_memo_undo` would do anything (otherwise its undo
    /// record need not be made).
    pub(crate) fn effects_memo_undo_wanted(&self) -> bool {
        DRY.get() || self.run.effects.recording()
    }

    pub(crate) fn effects_memo_undo(&self, undo: Arc<dyn Fn() + Send + Sync>) {
        if DRY.get() {
            undo();
            return;
        }
        self.run.effects.memo_undo(tlx_runtime::live::current_worker(), undo);
    }

    /// Record side effects (a restart of the run is possible) or not.
    pub fn effects_active(&self, on: bool) {
        self.run.effects.set_active(on);
    }

    /// The current thread, a worker of a search, starts expanding a level
    /// (a resumed search starts again from there).
    pub fn effects_mark(&self) {
        if let Some(w) = tlx_runtime::live::current_worker() {
            self.run.effects.mark(w, self.with_registers(|regs, _| regs.clone()));
        }
    }

    /// The current thread, a worker of a search, ends.
    pub fn effects_worker_end(&self) {
        if let Some(w) = tlx_runtime::live::current_worker() {
            self.run.effects.worker_end(w, self.with_registers(|regs, _| regs.clone()));
        }
    }

    /// The search resumes at the start of the level it was expanding:
    /// undo the effects of the workers that took part in that level
    /// (`rolled_back[w]`); the workers continue with their registers. An
    /// exit requested in that level is undone too (the level requests it
    /// again).
    pub fn effects_rollback_level(&self, rolled_back: &[bool]) -> Result<(), String> {
        self.run.effects.rollback_level(rolled_back).map_err(|e| e.to_string())?;
        self.run.exit.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// The run starts over: undo its effects (its output is not repeated,
    /// its files are restored, an exit it requested is undone; the caller
    /// restores the registers).
    pub fn effects_rollback_run(&self) -> Result<(), String> {
        self.run.effects.rollback_run().map_err(|e| e.to_string())?;
        self.run.exit.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// The run is over (no restart follows).
    pub fn effects_finish(&self) {
        self.run.effects.finish();
    }

    /// The compiled tier's pre-run is over: its effects are undone (as
    /// `effects_rollback_run`), the results of the programs it ran (IOExec)
    /// are kept for the search, which gets them instead of running the
    /// programs again. An exit it requested (`TLCSet("exit", TRUE)`) is
    /// undone: the search requests it where TLC does.
    pub fn effects_finish_prerun(&self) -> Result<(), String> {
        self.run.effects.finish_prerun().map_err(|e| e.to_string())?;
        self.run.exit.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Run the already prepared invocation once per uninterrupted evaluation,
    /// replaying its results across compiled-tier restarts (see `effects`).
    pub(crate) fn run_command(&self, command: crate::natives::PreparedCommand) -> crate::error::EResult<Value> {
        if DRY.get() {
            return command.run();
        }
        self.run.effects.run_command(&command, tlx_runtime::live::current_worker(), || command.run())
    }

    /// The main thread's registers (e.g. before a compiled tier's pre-run,
    /// whose evaluations may set registers).
    pub fn save_registers(&self) -> SavedRegisters {
        SavedRegisters(self.run.main_registers.lock().unwrap().clone())
    }

    /// Restore the main thread's registers saved by `save_registers`.
    pub fn restore_registers(&self, saved: &SavedRegisters) {
        *self.run.main_registers.lock().unwrap() = saved.0.clone();
    }

    /// Checkpoints: the registers of the current thread (a worker of the
    /// search), serialized (`write_registers`).
    pub fn registers_snapshot(&self) -> Result<Vec<u8>, String> {
        write_registers(&self.with_registers(|regs, _| regs.clone()))
    }

    /// Checkpoints: the main thread's registers (those a worker starts
    /// from), serialized.
    pub fn main_registers_snapshot(&self) -> Result<Vec<u8>, String> {
        write_registers(&self.run.main_registers.lock().unwrap())
    }

    /// Recovering from a checkpoint: the main thread's registers are
    /// `main` (`main_registers_snapshot`; empty: unchanged), worker `i` of
    /// the search starts from the registers `snaps[i]`
    /// (`registers_snapshot` of worker `i` of the checkpointed search;
    /// the others start from the main thread's).
    pub fn resume_registers(&self, main: &[u8], snaps: &[Vec<u8>]) -> Result<(), String> {
        if !main.is_empty() {
            *self.run.main_registers.lock().unwrap() = read_registers(main)?;
        }
        let resume = snaps.iter().map(|s| (!s.is_empty()).then(|| read_registers(s)).transpose()).collect::<Result<Vec<_>, String>>()?;
        self.run.effects.set_resume(resume);
        Ok(())
    }

    /// The shared copies of the registers of the workers that thread `me`
    /// may change: all if it is the main thread, else its own.
    pub(super) fn copies_of_registers(&self, main: bool, me: std::thread::ThreadId) -> Vec<(usize, Registers)> {
        let all = self.run.worker_regs.lock().unwrap();
        all.iter().enumerate().filter_map(|(w, e)| e.as_ref().filter(|(t, _)| main || *t == me).map(|(_, r)| (w, r.clone()))).collect()
    }

    pub(crate) fn get_register(&self, i: i64) -> Option<Value> {
        if std::thread::current().id() == self.run.main_thread {
            if let Some(Some((_, regs))) = self.run.worker_regs.lock().unwrap().first() {
                return regs.get(&i).cloned();
            }
        }
        self.with_registers(|regs, _| regs.get(&i).cloned())
    }

    pub(crate) fn set_register(&self, i: i64, v: Value) {
        let main = std::thread::current().id() == self.run.main_thread;
        if main {
            // (The main thread sets the registers of all workers.)
            for (_, regs) in self.run.worker_regs.lock().unwrap().iter_mut().flatten() {
                regs.insert(i, v.clone());
            }
        }
        self.with_registers(|regs, worker| {
            if let Some(w) = worker {
                let mut all = self.run.worker_regs.lock().unwrap();
                if let Some(Some((t, copy))) = all.get_mut(w) {
                    if *t == std::thread::current().id() {
                        copy.insert(i, v.clone());
                    }
                }
            }
            regs.insert(i, v);
        })
    }
}

#[cfg(test)]
mod run_state_tests {
    use super::*;
    use tlx_front::cfg::Config;
    use tlx_sema::Program;

    fn interp() -> Arc<Interp> {
        let dir = tlx_testkit::TempDir::new("interp-run-state");
        let path = dir.write("RunState.tla", &tlx_testkit::module("RunState", "EXTENDS Integers, TLC\n"));
        Interp::new(Program::load(&path, &[]).unwrap(), Config::default()).unwrap()
    }

    /// A record as `Value::record` builds it (the fields it reports).
    fn rec(fields: Vec<(&str, Value)>) -> Value {
        Value::record(fields.into_iter().map(|(k, v)| (tlx_value::Sym::new(k), v)).collect())
    }

    /// `TLCGet("stats")` of a finished search: the field names, order and
    /// values are TLC's statistics contract, so the moved `stats_record`
    /// and `sim_stats_record` must keep them exactly.
    #[test]
    fn stats_records_are_unchanged() {
        let search = RunStats { distinct: 1, generated: 2, queue: 3, diameter: 4, duration_secs: 5, initial: 6, ..Default::default() };
        let mut expected = vec![
            ("distinct", Value::Int(1)),
            ("generated", Value::Int(2)),
            ("queue", Value::Int(3)),
            ("diameter", Value::Int(4)),
            ("duration", Value::Int(5)),
            ("worker", Value::Int(0)),
            ("initial", Value::Int(6)),
        ];
        assert_eq!(Interp::stats_record(&search), rec(expected.clone()));
        expected.push(("traces", Value::Int(9)));
        assert_eq!(Interp::stats_record(&RunStats { traces: Some(9), ..search }), rec(expected));

        let empty = Value::record(Vec::new());
        let sim = RunStats {
            distinct: 1,
            generated: 2,
            queue: 3,
            diameter: 4,
            duration_secs: 5,
            initial: 6,
            traces: Some(9),
            level_mean: 8,
            level_variance: -7,
        };
        assert_eq!(
            Interp::sim_stats_record(&sim),
            rec(vec![
                ("traces", Value::Int(9)),
                ("duration", Value::Int(5)),
                ("generated", Value::Int(2)),
                ("behavior", rec(vec![("id", Value::Int(9)), ("actions", empty.clone())])),
                ("worker", Value::Int(0)),
                ("distinct", Value::Int(-1)),
                ("distinctvalues", Value::Int(-1)),
                ("retries", Value::Int(-1)),
                ("actions", empty),
                ("levelmean", Value::Int(8)),
                ("levelvariance", Value::Int(-7)),
            ])
        );
    }

    /// `set_final_stats` is what `TLCGet("stats")` reads after the search; a
    /// simulation records both the search and the simulation record.
    #[test]
    fn final_stats_round_trip() {
        let ip = interp();
        assert_eq!(ip.final_stats(), None);
        assert_eq!(ip.final_sim_stats(), None);
        let search = RunStats { distinct: 11, ..Default::default() };
        ip.set_final_stats(search);
        assert_eq!(ip.final_stats(), Some(Interp::stats_record(&search)));
        assert_eq!(ip.final_sim_stats(), None);
        let sim = RunStats { traces: Some(3), level_mean: 2, level_variance: 1, ..Default::default() };
        ip.set_final_stats(sim);
        assert_eq!(ip.final_stats(), Some(Interp::stats_record(&sim)));
        assert_eq!(ip.final_sim_stats(), Some(Interp::sim_stats_record(&sim)));
    }

    /// Register checkpoint snapshots: the byte form and its replay into a new
    /// run's start registers are the checkpoint contract, and the main
    /// thread's snapshot is also what this thread sees as its registers.
    #[test]
    fn checkpoint_register_snapshots_round_trip() {
        let ip = interp();
        ip.set_register(0, Value::Int(7));
        ip.set_register(1, Value::TRUE);
        let expected = Registers::from_iter([(0, Value::Int(7)), (1, Value::TRUE)]);
        let bytes = write_registers(&expected).unwrap();
        assert_eq!(ip.main_registers_snapshot().unwrap(), bytes);
        assert_eq!(ip.registers_snapshot().unwrap(), bytes);
        let recovered = interp();
        recovered.resume_registers(&ip.main_registers_snapshot().unwrap(), &[ip.registers_snapshot().unwrap()]).unwrap();
        assert_eq!(recovered.run.effects.take_resume_registers(0), Some(expected));
    }

    /// `save_registers`/`restore_registers` around a compiled tier's pre-run.
    #[test]
    fn saved_main_registers_restore() {
        let ip = interp();
        ip.set_register(0, Value::Int(7));
        let saved = ip.save_registers();
        ip.set_register(0, Value::Int(9));
        assert_eq!(ip.get_register(0), Some(Value::Int(9)));
        ip.restore_registers(&saved);
        assert_eq!(ip.get_register(0), Some(Value::Int(7)));
    }
}
