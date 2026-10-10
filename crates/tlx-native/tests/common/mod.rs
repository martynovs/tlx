//! Shared test helpers: a two-counter C model and a `Model` over a loaded
//! `NativeLib` (the runtime-facing half lives here so tlx-native itself
//! does not depend on tlx-runtime).
#![allow(dead_code)]

use tlx_native::native::NativeLib;
use tlx_runtime::{Model, ModelError};

/// A two-counter model with an invariant that can be made to fail (`BAD`).
pub const COUNTERS: &str = r#"
#include "tlx_abi.h"
#define N 30
static const char *acts[] = {"IncX", "IncY"};
static const char *invs[] = {"NotBad"};
static const char *errs[] = {"boom"};
static const tlx_spec_info info = {TLX_ABI_VERSION, 2, 2, 1, acts, invs, 1, errs};
const tlx_spec_info *tlx_info(void) { return &info; }
int tlx_init(const tlx_host *h, tlx_ctx *cx) {
  uint64_t s[2] = {0, 0};
  return h->emit(cx, s, 0) ? TLX_STOP : TLX_OK;
}
int tlx_next(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, uint32_t a) {
  uint64_t t[2] = {s[0], s[1]};
  if (s[a] >= N) return TLX_OK;
  t[a] += 1;
  return h->emit(cx, t, a) ? TLX_STOP : TLX_OK;
}
int tlx_invariant(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, uint32_t i) {
  return !(s[0] == 3 && s[1] == 4 && BAD);
}
"#;

/// Minimal model over a native library: states are word arrays, rendering
/// shows raw words. (The compiled model in tlx-compile adds decoding,
/// interpreter fallbacks and the rest of the configuration.)
pub struct NativeModel {
    pub lib: NativeLib,
}

impl Model for NativeModel {
    type State = Box<[u64]>;
    tlx_runtime::model_without_checks!();

    fn init_states(&self, out: &mut dyn FnMut(Box<[u64]>)) -> Result<(), ModelError> {
        let mut o = |s: Box<[u64]>, _a: u32| {
            out(s);
            true
        };
        self.lib.init_states(&mut o).map_err(ModelError::new)
    }

    fn successors(&self, s: &Box<[u64]>, out: &mut dyn FnMut(Box<[u64]>, u32) -> bool) -> Result<(), ModelError> {
        self.lib.successors(s, out).map_err(ModelError::new)
    }

    fn fingerprint(&self, s: &Box<[u64]>) -> u64 {
        tlx_native::fp_words(s)
    }

    fn check_invariants(&self, s: &Box<[u64]>) -> Result<Option<usize>, ModelError> {
        self.lib.check_invariants(s).map_err(ModelError::new)
    }

    fn invariant_names(&self) -> &[String] {
        self.lib.invariant_names()
    }

    fn action_names(&self) -> &[String] {
        self.lib.action_names()
    }

    fn render(&self, s: &Box<[u64]>) -> String {
        format!("{s:?}")
    }
}
