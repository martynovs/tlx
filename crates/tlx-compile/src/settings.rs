//! Tuning and debugging switches of the compiled tier: the `TLX_*`
//! environment variables, parsed once per process (`Settings::init`).
//!
//! Most compiled-tier knobs are read here, once. Some switches are read
//! where they apply (`verify.rs`, `enc/model.rs`, `jit/mod.rs`), through
//! `Env::process`. All the variables are developer knobs, documented in
//! `docs/configuration.md`.

use std::sync::OnceLock;
use std::time::Duration;

use tlx_base::knobs::Env;

/// Environment-controlled settings (all off / default when unset).
#[derive(Clone, Debug)]
pub struct Settings {
    /// `TLX_NO_NATIVE`: do not generate native code (run lowered IR only).
    pub no_native: bool,
    /// `TLX_PARANOID`: cross-check compiled results against the interpreter.
    pub paranoid: bool,
    /// `TLX_PRERUN=n`: number of distinct states of the shape pre-run.
    pub prerun_states: usize,
    /// `TLX_PRERUN_MS=n`: time budget of the shape pre-run.
    pub prerun_time: Duration,
    /// `TLX_PRERUN_SETTLE=n`: the shape pre-run stops once this many
    /// states (and twice as many as before) fitted the layout chosen so far
    /// (0: never; see `PrerunBudget::settle`).
    pub prerun_settle: usize,
    /// `TLX_PRERUN_REUSE` (default on; `0` turns it off; off with
    /// `TLX_PARANOID`): a faithful pre-run that explores the whole state
    /// space is the search (`enc::Checked::Interpreted`).
    pub prerun_reuse: bool,
    /// `TLX_NATIVE_SYNC` (implied by `TLX_PARANOID`): build the native
    /// code before exploring instead of in the background.
    pub native_sync: bool,
    /// `TLX_NO_MATERIALIZE`: do not materialize state-level definitions.
    pub no_materialize: bool,
    /// `TLX_VALUE_STORE` (experimental): slots whose shape would be `Dyn`
    /// hold handles into a run-owned value store (`Shape::Ref`,
    /// `tlx_layout::store`). The store is off with `TLX_ESCAPE` (the
    /// escape slots replace its layout) and for checkpoints (it has no
    /// checkpoint format yet): the driver decides that per run
    /// (`enc::driver`).
    pub value_store: bool,
    /// Experimental stable inline slots with action-local deoptimization.
    pub escape: bool,
    /// `TLX_VALUE_STORE=all` (testing): every slot does.
    pub value_store_all: bool,
    /// `TLX_MIXED`: mixed-mode native code (docs/mixed-mode.md): what C
    /// cannot compile is evaluated by the host as regions, instead of the
    /// whole action or predicate falling back. On by default (`0` turns it
    /// off): with adaptive tiering and memoized regions no measured model
    /// is slower (docs/mixed-mode.md).
    pub mixed: bool,
    /// `TLX_MIXED_FORCE=k` (testing; implies `TLX_MIXED`): also make every
    /// k-th expression C could compile a region (0: none).
    pub mixed_force: u32,
    /// `TLX_MIXED_ADAPT` (default on; `0` turns it off): per function,
    /// compare the time of mixed native calls with that of the fallback
    /// (IR executor or interpreter) on samples of the calls and use the
    /// faster (docs/mixed-mode.md §7). Off: mixed functions always run
    /// natively.
    pub mixed_adapt: bool,
    /// `TLX_RETAIN_MB=n`: memory for retaining the visited states while a
    /// layout miss is still possible, so that a restart continues the
    /// search instead of starting over (0: always start over).
    pub retain_mb: Option<usize>,
    /// `TLX_MAX_RESTARTS=n`: layout-miss restarts before the search
    /// continues with dynamic values (default: the caller's, 16).
    pub max_restarts: Option<usize>,
    /// `TLX_INJECT_MISMATCH=kind[:n]` (testing only): corrupt one compiled
    /// result, the `n`-th (from 0) eligible one, to check that
    /// `TLX_PARANOID` reports it. See `Inject`.
    pub inject: Option<Inject>,
    /// `TLX_MEMO_SLOTS=n`: memo table slots per thread (rounded up to a
    /// power of two).
    pub memo_slots: usize,
    /// `TLX_MEMO_MB=n`: memo byte budget of all threads (each thread of a
    /// search gets its share, see `exec::memo`).
    pub memo_mb: usize,
    /// `TLX_MEMO_STATS`: with `-stats`, report memo hit rates per interpreted
    /// expression.
    pub memo_stats: bool,
    /// `TLX_JIT`: the tiers of native code (docs/design.md §6.6).
    pub jit: Jit,
    /// `TLX_CC_DELAY_MS`: with `Jit::Tier`, how long the search runs on
    /// JIT code before the C library is built.
    pub cc_delay: Duration,
    /// `TLX_CC_AT_EXIT_MS`: a run that ends before the delayed C build
    /// started still starts it (without waiting) when the JIT compilation
    /// took at least this long (0: never).
    pub cc_at_exit: Duration,
}

/// Which native code `-compile` uses (`TLX_JIT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jit {
    /// `0`: the C library only.
    Off,
    /// `1` / `only`: the Cranelift JIT library only.
    Only,
    /// `tier` (the default with the `jit` feature): the JIT library first,
    /// then the C `-O2` library, which replaces it.
    Tier,
}

/// A deliberate wrong compiled result (`TLX_INJECT_MISMATCH`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inject {
    pub kind: InjectKind,
    /// Eligible results to leave alone before the corrupted one.
    pub after: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectKind {
    /// Drop the last successor of a compiled action run.
    Drop,
    /// Duplicate the last successor of a compiled action run.
    Dup,
    /// Replace the last successor of a compiled action run by its
    /// predecessor (a stale value).
    Stale,
    /// Drop the last state of compiled Init.
    Init,
    /// Negate the verdict of a native invariant.
    Inv,
    /// Where nothing restarts (after the search; also in the dynamic
    /// fallback layout): report a layout miss for a compiled action run or
    /// the liveness state atoms of a state.
    Miss,
}

impl Inject {
    /// Parse `kind[:n]`.
    pub fn parse(s: &str) -> Option<Inject> {
        let (k, n) = s.split_once(':').unwrap_or((s, "0"));
        let kind = match k {
            "drop" => InjectKind::Drop,
            "dup" => InjectKind::Dup,
            "stale" => InjectKind::Stale,
            "init" => InjectKind::Init,
            "inv" => InjectKind::Inv,
            "miss" => InjectKind::Miss,
            _ => return None,
        };
        Some(Inject { kind, after: n.parse().ok()? })
    }
}

impl Settings {
    /// The byte budget of the value store (`value_store`): the run's own
    /// memory budget, never a fraction of it. The store replaces the
    /// process-wide `Dyn` interner for its slots, and that interner had no
    /// limit of its own, so a budget below the run's would end a run the
    /// run's own memory guard lets finish. Without a budget (`-maxmem`
    /// unlimited), the store's page table is its only limit.
    pub fn value_store_bytes(&self, maxmem: usize) -> usize {
        maxmem
    }

    /// In-memory archive allowance; disk retains the rest when permitted.
    pub fn retain_bytes(&self, maxmem: usize) -> usize {
        self.retain_mb.map_or_else(|| if maxmem == usize::MAX { 1 << 30 } else { maxmem / 4 }, |mb| mb.saturating_mul(1 << 20))
    }

    /// Parse the `TLX_*` variables of the compiled tier (declared in
    /// `tlx_base::knobs`): numeric and structured values that do not parse
    /// are an error naming the variable.
    pub fn from_env() -> Result<Settings, String> {
        Settings::from_vars(Env::process())
    }

    /// `from_env` over an arbitrary variable lookup (for tests).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Settings, String> {
        Settings::from_vars(Env::lookup(&get))
    }

    fn from_vars(env: Env) -> Result<Settings, String> {
        let flag = |name: &str| env.flag(name);
        let paranoid = flag("TLX_PARANOID");
        let mixed_force =
            u32::try_from(env.num("TLX_MIXED_FORCE")?).map_err(|_| "invalid value of TLX_MIXED_FORCE (too large)".to_string())?;
        let jit = match env.choice("TLX_JIT")? {
            None => {
                if cfg!(feature = "jit") {
                    Jit::Tier
                } else {
                    Jit::Off
                }
            }
            Some("0" | "off") => Jit::Off,
            Some("1" | "only") => Jit::Only,
            Some(_) => Jit::Tier,
        };
        let inject = env.parsed("TLX_INJECT_MISMATCH", Inject::parse)?;
        // (TLX_ESCAPE is not combined with the store: it is the run's own
        // layout replacement, so the store is off with it; the driver
        // decides that per run and reports it.)
        let value_store = env.choice("TLX_VALUE_STORE")?;
        Ok(Settings {
            no_native: flag("TLX_NO_NATIVE"),
            paranoid,
            prerun_states: env.num("TLX_PRERUN")? as usize,
            prerun_time: Duration::from_millis(env.num("TLX_PRERUN_MS")?),
            prerun_settle: env.num("TLX_PRERUN_SETTLE")? as usize,
            prerun_reuse: flag("TLX_PRERUN_REUSE"),
            native_sync: flag("TLX_NATIVE_SYNC") || paranoid,
            no_materialize: flag("TLX_NO_MATERIALIZE"),
            value_store: matches!(value_store, Some("1" | "on" | "all")),
            escape: flag("TLX_ESCAPE"),
            value_store_all: value_store == Some("all"),
            mixed: flag("TLX_MIXED") || mixed_force > 0,
            mixed_force,
            mixed_adapt: flag("TLX_MIXED_ADAPT"),
            retain_mb: env.opt_num("TLX_RETAIN_MB")?.map(|n| n as usize),
            max_restarts: env.opt_num("TLX_MAX_RESTARTS")?.map(|n| n as usize),
            inject,
            memo_slots: env.num("TLX_MEMO_SLOTS")?.clamp(1, 1 << 26).next_power_of_two() as usize,
            memo_mb: env.num("TLX_MEMO_MB")?.max(1) as usize,
            memo_stats: flag("TLX_MEMO_STATS"),
            jit,
            cc_delay: Duration::from_millis(env.num("TLX_CC_DELAY_MS")?),
            cc_at_exit: env.millis("TLX_CC_AT_EXIT_MS")?,
        })
    }

    /// Parse the process-wide settings from the environment (once); a
    /// front end calls this at startup to report invalid values.
    pub fn init() -> Result<&'static Settings, String> {
        static GLOBAL: OnceLock<Result<Settings, String>> = OnceLock::new();
        GLOBAL.get_or_init(Settings::from_env).as_ref().map_err(Clone::clone)
    }

    /// The process-wide settings (see `init`).
    ///
    /// Panics if the environment holds an invalid value (a front end
    /// reports it with `init` before getting here).
    pub fn global() -> &'static Settings {
        Settings::init().unwrap_or_else(|e| panic!("{e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(vars: &[(&str, &str)]) -> Result<Settings, String> {
        Settings::from_lookup(|n| vars.iter().find(|(k, _)| *k == n).map(|(_, v)| v.to_string()))
    }

    #[test]
    fn archive_allowance_follows_budget_and_explicit_override() {
        let s = parse(&[]).unwrap();
        assert_eq!(s.retain_bytes(usize::MAX), 1 << 30);
        assert_eq!(s.retain_bytes(64 << 20), 16 << 20);
        assert_eq!(s.retain_bytes(8 << 30), 2 << 30);
        assert_eq!(parse(&[("TLX_RETAIN_MB", "0")]).unwrap().retain_bytes(8 << 30), 0);
        assert_eq!(parse(&[("TLX_RETAIN_MB", "7")]).unwrap().retain_bytes(64 << 20), 7 << 20);
    }

    #[test]
    fn defaults_and_values() {
        let s = parse(&[]).unwrap();
        assert!(!s.paranoid && !s.native_sync && s.inject.is_none());
        assert!(s.mixed && s.mixed_adapt);
        assert_eq!((s.prerun_states, s.prerun_time, s.prerun_settle, s.retain_mb), (20_000, Duration::from_millis(1000), 2000, None));
        assert_eq!(parse(&[("TLX_PRERUN_SETTLE", "0")]).unwrap().prerun_settle, 0);
        let s = parse(&[("TLX_PARANOID", "1"), ("TLX_PRERUN", "3"), ("TLX_INJECT_MISMATCH", "drop:7"), ("TLX_NO_NATIVE", "0")]).unwrap();
        assert!(s.paranoid && s.native_sync && !s.no_native);
        assert_eq!(s.prerun_states, 3);
        assert_eq!(parse(&[("TLX_MAX_RESTARTS", "2")]).unwrap().max_restarts, Some(2));
        assert!(parse(&[("TLX_MAX_RESTARTS", "x")]).is_err());
        assert_eq!(s.inject, Some(Inject { kind: InjectKind::Drop, after: 7 }));
        let s = parse(&[("TLX_MEMO_SLOTS", "1000")]).unwrap();
        assert!(!s.memo_stats);
        assert_eq!((s.memo_slots, s.memo_mb), (1024, 128));
        let s = parse(&[("TLX_MIXED_FORCE", "4")]).unwrap();
        assert!(s.mixed && s.mixed_force == 4);
        assert!(parse(&[("TLX_MIXED", "1")]).unwrap().mixed);
        assert!(!parse(&[("TLX_MIXED", "0")]).unwrap().mixed);
        assert!(parse(&[("TLX_MIXED", "0"), ("TLX_MIXED_FORCE", "2")]).unwrap().mixed);
        let s = parse(&[("TLX_MIXED_ADAPT", "0")]).unwrap();
        assert!(!s.mixed_adapt);
        let s = parse(&[]).unwrap();
        assert_eq!(
            (s.jit, s.cc_delay, s.cc_at_exit),
            (if cfg!(feature = "jit") { Jit::Tier } else { Jit::Off }, Duration::from_millis(1500), Duration::from_millis(200))
        );
        assert_eq!(parse(&[("TLX_JIT", "1")]).unwrap().jit, Jit::Only);
        assert_eq!(parse(&[("TLX_JIT", "0"), ("TLX_CC_DELAY_MS", "0")]).unwrap().cc_delay, Duration::ZERO);
        assert_eq!(parse(&[("TLX_CC_AT_EXIT_MS", "0")]).unwrap().cc_at_exit, Duration::ZERO);
        assert_eq!(parse(&[("TLX_CC_AT_EXIT_MS", "0.001")]).unwrap().cc_at_exit, Duration::from_micros(1));
        assert!(parse(&[("TLX_CC_AT_EXIT_MS", "-1")]).is_err());
        assert_eq!(parse(&[("TLX_JIT", "0")]).unwrap().jit, Jit::Off);
        assert_eq!(parse(&[("TLX_JIT", "tier")]).unwrap().jit, Jit::Tier);
    }

    #[test]
    fn the_value_store_switch_takes_off_on_and_all() {
        // Unset is the default: off (docs/configuration.md). The store stays
        // an experiment until reclamation and a checkpoint format exist.
        let s = parse(&[]).unwrap();
        assert!(!s.value_store && !s.value_store_all);
        for off in ["0", "off", "OFF"] {
            let s = parse(&[("TLX_VALUE_STORE", off)]).unwrap();
            assert!(!s.value_store && !s.value_store_all, "{off}");
        }
        for on in ["1", "on", "ON"] {
            let s = parse(&[("TLX_VALUE_STORE", on)]).unwrap();
            assert!(s.value_store && !s.value_store_all, "{on}");
        }
        let all = parse(&[("TLX_VALUE_STORE", "all")]).unwrap();
        assert!(all.value_store && all.value_store_all);
        assert!(parse(&[("TLX_VALUE_STORE", "yes")]).unwrap_err().contains("TLX_VALUE_STORE"));
        // The switch is independent of the escape experiment (the driver
        // turns the store off with it, rather than refusing the pair).
        let s = parse(&[("TLX_ESCAPE", "1"), ("TLX_VALUE_STORE", "1")]).unwrap();
        assert!(s.escape && s.value_store);
    }

    #[test]
    fn invalid_values() {
        let e = parse(&[("TLX_PRERUN_MS", "1s")]).unwrap_err();
        assert!(e.contains("TLX_PRERUN_MS") && e.contains("\"1s\""), "{e}");
        assert!(parse(&[("TLX_INJECT_MISMATCH", "flip")]).unwrap_err().contains("TLX_INJECT_MISMATCH"));
        assert!(parse(&[("TLX_RETAIN_MB", "-1")]).is_err());
        assert!(parse(&[("TLX_JIT", "yes")]).unwrap_err().contains("TLX_JIT"));
    }
}
