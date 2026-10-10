//! Shared helpers for tlx tests (dev-only).
//!
//! Always available (std, `tlx-base` and, for bounded subprocesses, Unix
//! `libc`):
//! - locating the workspace, `third_party/` and `corpus/out/` without
//!   hard-coded absolute paths, with an explicit skip/fail policy when they
//!   are missing (`require_third_party`, `require_corpus`);
//! - self-cleaning temporary directories (`TempDir`) that record their
//!   checkout owner for `scripts/tempdirs.py`, with non-precreated child
//!   paths (`TempDir::child_path`);
//! - a deterministic PRNG (`Rng`): the shared `tlx_base::rng::SplitMix64`
//!   core progression with testkit's own multiply-high bounded sampling.
//!
//! Model tiers are separately selectable so that a fixture acquires only the
//! layers it needs:
//! - `values`: random values of a layout `Shape`; no parser, evaluator,
//!   runtime or compiler;
//! - `interpreted`: `spec` (loading a spec from a string and a cfg, and
//!   running it in the interpreter tier) and `corpus` (bounded state
//!   sampling); adds `tlx-front`, `tlx-sema`, `tlx-eval` and `tlx-runtime`
//!   but not the compiled tier;
//! - `compiled`: additionally `spec`'s `-compile` tiers (`Mode::Compile`,
//!   `run_mode`, `assert_modes_agree`) and `diff`, the differential harness
//!   running lowered actions and predicates in the IR executor and as
//!   generated C on the same encoded states, with the interpreter as the
//!   reference.
//!
//! `models` is the aggregate of every model tier (`compiled`).
//!
//! # Missing external data
//! Tests that need `third_party/` (`scripts/fetch-deps.sh`) or the corpus
//! results (`corpus/out/`) call `require_third_party` / `require_corpus`.
//! When the data is missing they print a visible `SKIPPED` line (written
//! past the test harness's output capture) and return `None`; with
//! `TLX_REQUIRE_THIRD_PARTY=1` (or `CI` set) they panic instead, so that a
//! clean checkout cannot pass those tests vacuously.
//!
//! Locations can be overridden with `TLX_THIRD_PARTY` and `TLX_CORPUS_OUT`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use tlx_base::rng::SplitMix64;

pub mod cli;
mod rng;
pub use rng::XorShift64Star;
#[cfg(feature = "runtime")]
pub mod report;
#[cfg(feature = "runtime")]
pub mod toy;

#[cfg(feature = "values")]
pub mod values;

#[cfg(feature = "interpreted")]
pub mod corpus;
#[cfg(feature = "interpreted")]
pub mod spec;

#[cfg(feature = "compiled")]
pub mod diff;

// ---------------------------------------------------------------------------
// Locations

/// The workspace root (the directory of the top-level Cargo.toml).
pub fn workspace_root() -> PathBuf {
    // crates/tlx-testkit -> workspace
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    p.canonicalize().unwrap_or(p)
}

/// `third_party/` (or `$TLX_THIRD_PARTY`), if it exists.
pub fn third_party() -> Option<PathBuf> {
    let p = std::env::var_os("TLX_THIRD_PARTY").map(PathBuf::from).unwrap_or_else(|| workspace_root().join("third_party"));
    p.is_dir().then_some(p)
}

/// `corpus/out/` (or `$TLX_CORPUS_OUT`), if it exists.
pub fn corpus_out() -> Option<PathBuf> {
    let p = std::env::var_os("TLX_CORPUS_OUT").map(PathBuf::from).unwrap_or_else(|| workspace_root().join("corpus/out"));
    p.is_dir().then_some(p)
}

/// Whether missing external test data must fail the test
/// (`TLX_REQUIRE_THIRD_PARTY` set to anything but `0`, or `CI` set).
pub fn data_required() -> bool {
    std::env::var("TLX_REQUIRE_THIRD_PARTY").is_ok_and(|v| v != "0") || std::env::var_os("CI").is_some()
}

/// Report that `test` cannot run because `missing` is absent: panic if
/// external data is required (`data_required`), else print a visible
/// `SKIPPED` line.
pub fn skip(test: &str, missing: &str) {
    let msg = format!("{test}: {missing} not found (run scripts/fetch-deps.sh)");
    if data_required() {
        panic!("{msg}; required by TLX_REQUIRE_THIRD_PARTY / CI");
    }
    note(&format!("SKIPPED {msg}"));
}

/// Print a line that the test harness does not capture (`eprintln!` output
/// of a passing test is swallowed).
pub fn note(line: &str) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "{line}");
}

/// `third_party/<rel>` for test `test`; None (after `skip`) if missing.
pub fn require_third_party(test: &str, rel: &str) -> Option<PathBuf> {
    let p = third_party().map(|t| t.join(rel)).filter(|p| p.exists());
    if p.is_none() {
        skip(test, &format!("third_party/{rel}"));
    }
    p
}

/// `corpus/out/<rel>` for test `test`; None (after `skip`) if missing.
pub fn require_corpus(test: &str, rel: &str) -> Option<PathBuf> {
    let p = corpus_out().map(|t| t.join(rel)).filter(|p| p.exists());
    if p.is_none() {
        skip(test, &format!("corpus/out/{rel}"));
    }
    p
}

/// The search path for modules of the community / tlapm / apalache
/// libraries (those of them that exist).
pub fn library_paths() -> Vec<PathBuf> {
    let Some(tp) = third_party() else { return Vec::new() };
    ["community", "tlapm/library", "apalache"].iter().map(|d| tp.join(d)).filter(|p| p.is_dir()).collect()
}

/// Re-root a path recorded by the corpus scripts (absolute, in the checkout
/// that produced it) at this checkout's `third_party/`: the part after the
/// last `third_party/` component is kept. Other paths are returned as is.
pub fn rebase_third_party(recorded: &str) -> PathBuf {
    match (recorded.rfind("/third_party/"), third_party()) {
        (Some(i), Some(tp)) => tp.join(&recorded[i + "/third_party/".len()..]),
        _ => PathBuf::from(recorded),
    }
}

// ---------------------------------------------------------------------------
// Temporary directories

/// The marker file that names the checkout owning a temporary directory.
/// `scripts/tempdirs.py` reports a leftover directory only when this file
/// records the checked worktree as its root, so a worker never claims or
/// deletes another worker's directories. Keep the name and the `root` / `pid`
/// keys in sync with `scripts/tempdirs.py`.
const OWNER_MARKER: &str = ".tlx-temp-owner.json";

/// A JSON string literal for `s` (the marker is parsed by
/// `scripts/tempdirs.py`; the always-available part of this crate has no
/// JSON dependency).
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Record `dir` as owned by this checkout (best effort: an unwritable marker
/// only weakens leftover attribution, it never fails the test).
fn write_owner_marker(dir: &Path) {
    let root = workspace_root();
    let body = format!("{{\"root\":{}, \"pid\":{}}}", json_string(&root.to_string_lossy()), std::process::id());
    let _ = std::fs::write(dir.join(OWNER_MARKER), body);
}

/// A fresh temporary directory with an ownership marker, removed on drop
/// (unless `TLX_KEEP_TMP` is set, for debugging).
pub struct TempDir {
    path: PathBuf,
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        self.path()
    }
}

impl TempDir {
    pub fn with_spec(tag: &str, src: &str, cfg: &str) -> Self {
        let dir = Self::new(tag);
        let name = module_name(src).expect("no MODULE header");
        dir.write(&format!("{name}.tla"), src);
        dir.write(&format!("{name}.cfg"), cfg);
        dir
    }

    pub fn write_modules(&self, modules: &[(&str, &str)]) -> PathBuf {
        for (name, src) in modules {
            self.write(&format!("{name}.tla"), src);
        }
        self.path.join(format!("{}.tla", modules[0].0))
    }

    /// A new empty directory named after `tag`.
    pub fn new(tag: &str) -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("tlx-test-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
        let dir = TempDir { path };
        write_owner_marker(dir.path());
        dir
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A path inside this directory that does not exist yet, for a test whose
    /// subject must create the file or directory itself: the parent stays
    /// owned (and removed on drop) without precreating what tlx has to create.
    pub fn child_path(&self, name: &str) -> PathBuf {
        let path = self.path.join(name);
        assert!(!path.exists(), "child_path {} already exists", path.display());
        path
    }

    /// Write `contents` to `name` in this directory; returns its path.
    pub fn write(&self, name: &str, contents: &str) -> PathBuf {
        let p = self.path.join(name);
        std::fs::write(&p, contents).unwrap_or_else(|e| panic!("write {}: {e}", p.display()));
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if std::env::var_os("TLX_KEEP_TMP").is_none() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// The text of module `name` with body `body`.
pub fn module(name: &str, body: &str) -> String {
    format!("---- MODULE {name} ----\n{body}\n====\n")
}

/// The name of the first module declared in `src` (`---- MODULE Name ----`).
pub fn module_name(src: &str) -> Option<&str> {
    let i = src.find("MODULE")?;
    src[i + "MODULE".len()..].split_whitespace().next()
}

// ---------------------------------------------------------------------------
// PRNG

/// Deterministic PRNG for property tests.
///
/// The raw progression is the shared [`SplitMix64`] core; the bounded
/// sampling transforms below are testkit's own multiply-high policy and are
/// pinned by golden tests, so a seeded sequence only changes with an explicit
/// behavior change (CODE-14). Kept distinct from the modulo-sampling
/// `tlx_base::rng::SplitMix64::below` and from `XorShift64Star`.
#[derive(Clone, Debug)]
pub struct Rng(SplitMix64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(SplitMix64::new(seed))
    }

    /// The next value of the shared SplitMix64 progression.
    pub fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    /// Uniform in `0..n` (n > 0) by multiply-high reduction.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "Rng::below(0)");
        ((self.next_u64() as u128 * n as u128) >> 64) as u64
    }

    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        assert!(lo <= hi);
        let span = (hi as i128 - lo as i128 + 1) as u128;
        if span > u64::MAX as u128 {
            return self.next_u64() as i64;
        }
        (lo as i128 + self.below(span as u64) as i128) as i64
    }

    /// True with probability `num/den`.
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }
}

/// Seed for property tests: `$TLX_SEED` if set, else `default`. Tests print
/// the seed they use so that a failure can be replayed.
pub fn seed(default: u64) -> u64 {
    std::env::var("TLX_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_spec_and_modules() {
        let first = TempDir::with_spec("modules", &module("A", "EXTENDS B"), "INIT Init\n");
        let second = TempDir::new("modules");
        assert_ne!(first.path(), second.path());
        assert_eq!(std::fs::read_to_string(first.path().join("A.cfg")).unwrap(), "INIT Init\n");
        let root = first.write_modules(&[("B", "body"), ("C", "other")]);
        assert_eq!(std::fs::read_to_string(root).unwrap(), "body");
        assert_eq!(std::fs::read_to_string(first.path().join("C.tla")).unwrap(), "other");
    }

    #[test]
    fn temporary_directory_records_its_owner() {
        let dir = TempDir::new("owner");
        let text = std::fs::read_to_string(dir.path().join(OWNER_MARKER)).expect("owner marker");
        assert!(text.contains(&format!("\"root\":{}", json_string(&workspace_root().to_string_lossy()))), "{text}");
        assert!(text.contains(&format!("\"pid\":{}", std::process::id())), "{text}");
    }

    #[test]
    fn child_path_is_inside_and_not_precreated() {
        let dir = TempDir::new("child");
        let child = dir.child_path("states");
        assert_eq!(child, dir.path().join("states"));
        assert!(!child.exists());
        std::fs::create_dir(&child).unwrap();
        assert!(child.is_dir());
    }

    #[test]
    fn temporary_directory_is_removed_when_its_test_panics() {
        let dir = TempDir::new("panic");
        let path = dir.path().to_path_buf();
        let joined = std::thread::spawn(move || {
            let _dir = dir;
            panic!("intentional panic: the temp directory must still be removed");
        });
        assert!(joined.join().is_err());
        assert!(!path.exists(), "{} survived the panic", path.display());
    }

    #[test]
    fn rng_raw_sequence_is_stable() {
        // Raw outputs before the refactor onto the shared SplitMix64 core,
        // including the zero and full seeds.
        for (seed, expected) in [
            (0u64, [0xe220_a839_7b1d_cdaf, 0x6e78_9e6a_a1b9_65f4, 0x06c4_5d18_8009_454f]),
            (7, [0x63cb_e1e4_5932_0dd7, 0x044c_3cd7_f43c_661c, 0xe698_4080_bab1_2a02]),
            (u64::MAX, [0xe4d9_7177_1b65_2c20, 0xe99f_f867_dbf6_82c9, 0x382f_f84c_b272_81e9]),
        ] {
            let mut rng = Rng::new(seed);
            let got: Vec<u64> = (0..3).map(|_| rng.next_u64()).collect();
            assert_eq!(got, expected, "seed {seed}");
        }
    }

    #[test]
    fn rng_reuses_the_shared_splitmix_core() {
        // The raw progression is exactly `tlx_base::rng::SplitMix64`; only the
        // bounded sampling below is testkit's own.
        for seed in [0, 1, 7, 0x9e37_79b9_7f4a_7c15, u64::MAX] {
            let mut rng = Rng::new(seed);
            let mut core = SplitMix64::new(seed);
            for _ in 0..8 {
                assert_eq!(rng.next_u64(), core.next_u64(), "seed {seed}");
            }
        }
    }

    #[test]
    fn rng_below_goldens_and_boundaries() {
        // Multiply-high reduction (not modulo), one golden per call, then the
        // extreme bounds: 1 always yields 0, `u64::MAX` keeps the high word.
        let mut rng = Rng::new(7);
        assert_eq!([rng.below(1), rng.below(2), rng.below(10), rng.below(7), rng.below(u64::MAX)], [0, 0, 9, 4, 8_346_079_845_500_723_673]);
        let mut rng = Rng::new(7);
        for _ in 0..200 {
            assert_eq!(rng.below(1), 0);
        }
        let mut rng = Rng::new(0);
        for bound in [1, 2, 3, 7, 100, u64::MAX] {
            assert!(rng.below(bound) < bound, "bound {bound}");
        }
    }

    #[test]
    fn rng_range_and_chance_goldens() {
        // One golden per transform, with the range boundaries: empty span
        // (`lo == hi`), a negative lower bound, and the full `i64` span that
        // overflows a `u64` count and falls back to the raw stream.
        let mut rng = Rng::new(7);
        assert_eq!([rng.range(-10, 10), rng.range(0, 1), rng.range(0, 3), rng.range(0, 3)], [-2, 0, 3, 2]);
        let mut rng = Rng::new(7);
        assert_eq!([rng.range(5, 5), rng.range(i64::MIN, i64::MAX)], [5, 0x044c_3cd7_f43c_661cu64 as i64]);
        let mut rng = Rng::new(7);
        assert_eq!([rng.chance(7, 10), rng.chance(0, 10), rng.chance(10, 10)], [true, false, true]);
    }

    #[test]
    fn rng_resumes_identically_after_clone() {
        let mut rng = Rng::new(7);
        for _ in 0..2 {
            rng.next_u64();
        }
        let mut resumed = rng.clone();
        for _ in 0..8 {
            assert_eq!(resumed.next_u64(), rng.next_u64());
        }
    }

    #[test]
    #[should_panic(expected = "Rng::below(0)")]
    fn rng_below_zero_panics() {
        Rng::new(0).below(0);
    }

    #[test]
    fn rng_is_deterministic_and_in_range() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        for _ in 0..1000 {
            let x = a.range(-3, 5);
            assert_eq!(x, b.range(-3, 5));
            assert!((-3..=5).contains(&x));
        }
    }

    #[test]
    fn module_names() {
        assert_eq!(module_name(&module("Foo", "VARIABLE x")), Some("Foo"));
        assert_eq!(module_name("nothing"), None);
    }

    #[test]
    fn rebase() {
        if let Some(tp) = third_party() {
            assert_eq!(rebase_third_party("/elsewhere/tlx/third_party/Examples/x.tla"), tp.join("Examples/x.tla"));
        }
        assert_eq!(rebase_third_party("/a/b.tla"), PathBuf::from("/a/b.tla"));
    }
}
