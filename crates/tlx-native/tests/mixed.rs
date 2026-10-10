//! Mixed mode (ABI v3): a hand-written C library calling the host's region
//! callbacks (`eval`, `elems`, `mark`, `release`) against a stub region
//! table; error and panic paths, stale handles, and the load-time checks
//! (ABI version, region table hash).

use std::panic::{catch_unwind, AssertUnwindSafe};

use tlx_native::build::{build, BuildOptions};
use tlx_native::mixed::{Args, In, Kind, MixedCx, Out, RegionEnv, RegionError, RegionSig, Regions};
use tlx_native::native::{Mixed, NativeLib, ERR, OK};
use tlx_value::Value;

/// Hash the library below claims for its region table.
const HASH: u64 = 0x0123_4567_89ab_cdef;

/// Two words per state (x, y). Actions:
/// 0. for each e in x..x+2 (region 0, a set handle; `elems`), emit (x, e*e)
///    with e*e from region 1 on the element handle, inside mark/release
///    (checked: the arena is back at the mark after each iteration);
/// 1. uses a handle after releasing it (stale);
/// 2. uses a handle saved by the previous call of action 2 (stale on the
///    second call; the C global is shared by all tests loading the library,
///    so only one test may run action 2);
/// 3. region 3 on x: fails for x < 0, panics for x = 99, else emits (x, 0);
/// 4. `elems` on a handle that is not a set (fails).
///    Invariant 0: region 2 swaps the words of the state given as an encoded
///    argument (and checks it sees s0 and no s1); holds iff the swap is right.
///    Action property 0: region 5 returns the call's level; holds iff it is 7
///    and region 2 sees both states.
const LIB: &str = r#"
#include "tlx_abi.h"
static const char *acts[] = {"Squares", "UseReleased", "UseOld", "Fallible", "ElemsOfInt"};
static const char *invs[] = {"Swap"};
static const char *errs[] = {"boom"};
static const tlx_spec_info info = {ABI, 2, 5, 1, acts, invs, 1, errs, NREGIONS, HASHULL};
const tlx_spec_info *tlx_info(void) { return &info; }
int tlx_init(const tlx_host *h, tlx_ctx *cx) {
  uint64_t s[2] = {0, 0};
  return h->emit(cx, s, 0) ? TLX_STOP : TLX_OK;
}
static uint64_t saved;
int tlx_next(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, uint32_t a) {
  uint64_t set, out, t[2];
  switch (a) {
  case 0: {
    tlx_arg a0[2] = {{.i = (int64_t)s[0]}, {.i = (int64_t)s[0] + 2}};
    if (h->eval(cx, 0, s, 0, a0, &set)) return TLX_ERR;
    uint64_t f, n;
    if (h->elems(cx, set, 0, &f, &n)) return TLX_ERR;
    for (uint64_t k = 0; k < n; k++) {
      uint64_t m = h->mark(cx);
      tlx_arg a1[1] = {{.h = f + k}};
      int64_t sq;
      if (h->eval(cx, 1, s, 0, a1, (uint64_t *)&sq)) return TLX_ERR;
      /* a temporary handle, released below */
      if (h->eval(cx, 0, s, 0, a0, &out)) return TLX_ERR;
      h->release(cx, m);
      if (h->mark(cx) != m) return -7;
      t[0] = s[0]; t[1] = (uint64_t)sq;
      if (h->emit(cx, t, 0)) return TLX_STOP;
    }
    return TLX_OK;
  }
  case 1: {
    tlx_arg a0[2] = {{.i = 1}, {.i = 2}};
    uint64_t m = h->mark(cx);
    if (h->eval(cx, 0, s, 0, a0, &set)) return TLX_ERR;
    h->release(cx, m);
    /* reuses the released slot: the old handle must not see the new value */
    if (h->eval(cx, 0, s, 0, a0, &out)) return TLX_ERR;
    tlx_arg a1[1] = {{.h = set}};
    if (h->eval(cx, 1, s, 0, a1, &out)) return TLX_ERR;
    return TLX_OK;
  }
  case 2: {
    tlx_arg a0[2] = {{.i = 1}, {.i = 2}};
    if (saved) {
      tlx_arg a1[1] = {{.h = saved}};
      if (h->eval(cx, 1, s, 0, a1, &out)) return TLX_ERR;
    }
    if (h->eval(cx, 0, s, 0, a0, &saved)) return TLX_ERR;
    return TLX_OK;
  }
  case 3: {
    tlx_arg a0[1] = {{.i = (int64_t)s[0]}};
    if (h->eval(cx, 3, s, 0, a0, &out)) return TLX_ERR;
    t[0] = s[0]; t[1] = out;
    return h->emit(cx, t, 3) ? TLX_STOP : TLX_OK;
  }
  case 4: {
    tlx_arg a0[1] = {{.i = 5}};
    uint64_t f, n;
    if (h->eval(cx, 4, s, 0, a0, &out)) return TLX_ERR;
    if (h->elems(cx, out, 1, &f, &n)) return TLX_ERR;
    return TLX_OK;
  }
  }
  return TLX_OK;
}
int tlx_invariant(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, uint32_t i) {
  uint64_t out[2];
  tlx_arg a0[1] = {{.w = s}};
  if (h->eval(cx, 2, s, 0, a0, out)) return TLX_ERR;
  return out[0] == s[1] && out[1] == s[0];
}
int tlx_action_property(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, const uint64_t *t, uint32_t i) {
  uint64_t lvl, out[2];
  tlx_arg a0[1] = {{.w = t}};
  if (h->eval(cx, 5, s, t, 0, &lvl)) return TLX_ERR;
  if (h->eval(cx, 2, s, t, a0, out)) return TLX_ERR;
  return lvl == 7 && out[0] == t[1];
}
"#;

/// The host side of the library's six regions.
struct Table {
    sigs: Vec<RegionSig>,
    hash: u64,
}

fn table(hash: u64) -> Table {
    use Kind::*;
    let sig = |inputs: Vec<Kind>, result| RegionSig { inputs, result };
    let sigs = vec![
        sig(vec![Scalar, Scalar], Handle),
        sig(vec![Handle], Scalar),
        sig(vec![Words(2)], Words(2)),
        sig(vec![Scalar], Scalar),
        sig(vec![Scalar], Handle),
        sig(vec![], Scalar),
    ];
    Table { sigs, hash }
}

impl Regions for Table {
    fn len(&self) -> u32 {
        self.sigs.len() as u32
    }

    fn hash(&self) -> u64 {
        self.hash
    }

    fn sig(&self, rid: u32) -> &RegionSig {
        &self.sigs[rid as usize]
    }

    fn eval(&self, rid: u32, args: &Args<'_>, env: &mut RegionEnv<'_>) -> Result<Out, RegionError> {
        let int = |i: usize| match args.get(i) {
            In::Int(x) => x,
            other => panic!("not a scalar: {other:?}"),
        };
        Ok(match rid {
            0 => Out::Val(Value::Interval(int(0), int(1))),
            1 => match args.get(0) {
                In::Val(Value::Int(e)) => Out::Int(e * e),
                In::Val(v) => return Err(RegionError(format!("not an integer: {v}"))),
                other => panic!("not a handle: {other:?}"),
            },
            2 => {
                let In::Words(w) = args.get(0) else { panic!("not an encoding") };
                let s0 = env.s0.expect("s0");
                assert_eq!(env.s1.is_some(), w.as_ptr() != s0.as_ptr(), "s1 given iff the argument is the next state");
                // Decoded through the state cache (once per state).
                let vals = env.cache.get(s0, |s| s.iter().map(|x| Value::Int(*x as i64)).collect());
                assert_eq!(vals.len(), 2);
                Out::Words(vec![w[1], w[0]].into())
            }
            3 => match int(0) {
                x if x < 0 => return Err(RegionError("negative".into())),
                99 => panic!("region 3 panicked"),
                x => Out::Int(x * 10),
            },
            4 => Out::Val(Value::Int(int(0))),
            5 => Out::Int(env.level as i64),
            _ => unreachable!(),
        })
    }
}

fn source(abi: &str, nregions: u32) -> String {
    LIB.replace("ABI", abi).replace("NREGIONS", &nregions.to_string()).replace("HASH", &format!("{HASH:#x}"))
}

fn lib_path(abi: &str, nregions: u32) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tlx-native-mixed-{}", std::process::id()));
    build(&source(abi, nregions), &BuildOptions { cache_dir: dir, ..Default::default() }).expect("build")
}

fn load() -> (NativeLib, Table) {
    let t = table(HASH);
    // SAFETY: LIB has static metadata and two-word buffers; callback slots
    // match table(). Version/count/hash variants are rejected before calls.
    // Only handle_of_an_ended_call_is_refused touches saved, on one thread;
    // other entry points are reentrant. Compiler/cache are trusted.
    let lib = unsafe { NativeLib::load_with(&lib_path("TLX_ABI_VERSION", 6), Some(&t), None) }.expect("load");
    (lib, t)
}

fn next(lib: &NativeLib, t: &Table, cx: &mut MixedCx, s: &[u64], a: u32) -> (i32, Vec<Box<[u64]>>) {
    let mut out = Vec::new();
    let rc = lib.next_mixed(Mixed { cx, regions: t, level: 3 }, s, a, &mut out);
    (rc, out)
}

fn panic_message(r: std::thread::Result<impl Sized>) -> String {
    let p = r.err().expect("the call must panic");
    p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
}

#[test]
fn eval_elems_mark_release() {
    let (lib, t) = load();
    let mut cx = MixedCx::new();
    let (rc, out) = next(&lib, &t, &mut cx, &[1, 0], 0);
    assert_eq!(rc, OK);
    assert_eq!(out, vec![vec![1u64, 1].into_boxed_slice(), vec![1, 4].into(), vec![1, 9].into()]);
    // 1 set + 3 squares + 3 temporaries.
    assert_eq!(cx.evals, 7);
    assert_eq!(cx.errors, 0);
    assert_eq!(cx.live_values(), 0, "the arena is emptied after the call");
    // Init needs no regions but runs through the mixed interface too.
    let mut init = Vec::new();
    assert_eq!(lib.init_mixed(Mixed { cx: &mut cx, regions: &t, level: 0 }, &mut init), OK);
    assert_eq!(init, vec![vec![0u64, 0].into_boxed_slice()]);
}

#[test]
fn encoded_arguments_results_and_state_cache() {
    let (lib, t) = load();
    let mut cx = MixedCx::new();
    for _ in 0..3 {
        assert_eq!(lib.invariant_mixed(Mixed { cx: &mut cx, regions: &t, level: 1 }, &[5, 6], 0), 1);
    }
    assert_eq!(cx.cache.decodes, 1, "one decoding per state");
    assert_eq!(lib.invariant_mixed(Mixed { cx: &mut cx, regions: &t, level: 1 }, &[6, 5], 0), 1);
    assert_eq!(cx.cache.decodes, 2);
    // Level and s1 reach the regions of an action property.
    assert_eq!(lib.action_property_mixed(Mixed { cx: &mut cx, regions: &t, level: 7 }, &[1, 2], &[3, 4], 0), 1);
    assert_eq!(lib.action_property_mixed(Mixed { cx: &mut cx, regions: &t, level: 6 }, &[1, 2], &[3, 4], 0), 0);
}

#[test]
fn region_error_fails_the_call() {
    let (lib, t) = load();
    let mut cx = MixedCx::new();
    let (rc, out) = next(&lib, &t, &mut cx, &[u64::MAX, 0], 3);
    assert_eq!((rc, out.len()), (ERR, 0));
    assert_eq!((cx.errors, cx.last_error.as_deref()), (1, Some("negative")));
    let (rc, out) = next(&lib, &t, &mut cx, &[2, 0], 3);
    assert_eq!((rc, out), (OK, vec![vec![2u64, 20].into_boxed_slice()]));
    // `elems` on a non-set fails the same way.
    let (rc, _) = next(&lib, &t, &mut cx, &[0, 0], 4);
    assert_eq!(rc, ERR);
    assert_eq!(cx.errors, 2);
    assert!(cx.last_error.as_deref().unwrap_or("").starts_with("Attempted to enumerate a non-set value"), "{:?}", cx.last_error);
    assert_eq!(cx.live_values(), 0);
}

#[test]
fn region_panic_propagates() {
    let (lib, t) = load();
    let mut cx = MixedCx::new();
    let r = catch_unwind(AssertUnwindSafe(|| next(&lib, &t, &mut cx, &[99, 0], 3)));
    assert_eq!(panic_message(r), "region 3 panicked");
    assert_eq!(cx.live_values(), 0);
    // The library and the context are still usable afterwards.
    let (rc, out) = next(&lib, &t, &mut cx, &[1, 0], 0);
    assert_eq!((rc, out.len()), (OK, 3));
}

#[test]
fn released_handle_is_refused() {
    let (lib, t) = load();
    let mut cx = MixedCx::new();
    let r = catch_unwind(AssertUnwindSafe(|| next(&lib, &t, &mut cx, &[0, 0], 1)));
    assert!(panic_message(r).contains("stale or invalid value handle"));
}

#[test]
fn handle_of_an_ended_call_is_refused() {
    let (lib, t) = load();
    let mut cx = MixedCx::new();
    assert_eq!(next(&lib, &t, &mut cx, &[0, 0], 2).0, OK);
    let r = catch_unwind(AssertUnwindSafe(|| next(&lib, &t, &mut cx, &[0, 0], 2)));
    assert!(panic_message(r).contains("stale or invalid value handle"));
}

#[test]
fn abi_version_mismatch_is_refused() {
    let t = table(HASH);
    // SAFETY: LIB has static metadata and two-word buffers; callback slots
    // match table(). Version/count/hash variants are rejected before calls.
    // Only handle_of_an_ended_call_is_refused touches saved, on one thread;
    // other entry points are reentrant. Compiler/cache are trusted.
    let err = unsafe { NativeLib::load_with(&lib_path("2u", 6), Some(&t), None) }.err().expect("an ABI v2 library must be refused");
    assert!(err.contains("ABI version mismatch: library 2 vs host 5"), "{err}");
}

#[test]
fn region_table_mismatch_is_refused() {
    let path = lib_path("TLX_ABI_VERSION", 6);
    // SAFETY: LIB has static metadata and two-word buffers; callback slots
    // match table(). Version/count/hash variants are rejected before calls.
    // Only handle_of_an_ended_call_is_refused touches saved, on one thread;
    // other entry points are reentrant. Compiler/cache are trusted.
    let err = unsafe { NativeLib::load_with(&path, Some(&table(HASH ^ 1)), None) }.err().expect("another region table must be refused");
    assert!(err.contains("region table mismatch"), "{err}");
    // SAFETY: LIB has static metadata and two-word buffers; callback slots
    // match table(). Version/count/hash variants are rejected before calls.
    // Only handle_of_an_ended_call_is_refused touches saved, on one thread;
    // other entry points are reentrant. Compiler/cache are trusted.
    let err = unsafe { NativeLib::load(&path) }.err().expect("a library with regions needs a region table");
    assert!(err.contains("no region table"), "{err}");
    let other = lib_path("TLX_ABI_VERSION", 5);
    // SAFETY: LIB has static metadata and two-word buffers; callback slots
    // match table(). Version/count/hash variants are rejected before calls.
    // Only handle_of_an_ended_call_is_refused touches saved, on one thread;
    // other entry points are reentrant. Compiler/cache are trusted.
    let err = unsafe { NativeLib::load_with(&other, Some(&table(HASH)), None) }.err().expect("another region count must be refused");
    assert!(err.contains("region table mismatch"), "{err}");
}

/// A table other than the one the library was loaded for is refused on
/// every call too.
#[test]
#[should_panic(expected = "is not the one the library was loaded for")]
fn call_with_another_table_is_refused() {
    let (lib, _) = load();
    let other = table(HASH ^ 1);
    let mut cx = MixedCx::new();
    let _ = next(&lib, &other, &mut cx, &[0, 0], 0);
}
