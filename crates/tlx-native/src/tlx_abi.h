/* tlx C ABI (design.md §6.7, mixed mode: mixed-mode.md §2). Generated model
 * code implements the functions declared at the end; the Rust host provides
 * the callbacks in tlx_host. States are arrays of `words` uint64_t (layout
 * chosen by the compiler). */
#ifndef TLX_ABI_H
#define TLX_ABI_H
#include <stddef.h>
#include <stdint.h>

#define TLX_ABI_VERSION 5u

/* Return codes of generated functions. */
#define TLX_OK 0
#define TLX_STOP 1   /* emit asked to stop */
#define TLX_ERR (-1) /* runtime error reported through host->error */
#define TLX_MISS (-2) /* layout miss reported through host->miss */

typedef struct tlx_ctx tlx_ctx; /* host-owned, per call */

/* A value passed to a host region: a scalar (Bool/Int), an encoding (pointer
   to the words of the input's shape) or a value handle. The meaning of each
   slot is fixed per region by the host's region table. */
typedef union { int64_t i; const uint64_t *w; uint64_t h; } tlx_arg;

typedef struct {
  /* A generated state (copied by the host). Nonzero return = stop. */
  int (*emit)(tlx_ctx *cx, const uint64_t *state, uint32_t action);
  /* Runtime error; err_id indexes tlx_spec_info.error_msgs. */
  void (*error)(tlx_ctx *cx, uint32_t err_id);
  /* A value did not fit the layout of variable `var` (speculation failed). */
  void (*miss)(tlx_ctx *cx, uint32_t var);
  /* --- v3: mixed mode (boxed regions evaluated by the host) --- */
  /* Evaluate region `rid`. s0: the current state (NULL in Init); s1: the
     next-state buffer in scope (NULL in state predicates). The result goes
     to `out` by the region's result type: Bool/Int -> out[0] (int64);
     encoding -> its words; boxed value -> out[0] = handle. Returns TLX_OK or
     TLX_ERR (the host re-runs the whole call in the interpreter, which
     reports the error). */
  int (*eval)(tlx_ctx *cx, uint32_t rid, const uint64_t *s0, const uint64_t *s1,
              const tlx_arg *args, uint64_t *out);
  /* The elements of the set handle `set` (TLC enumeration order, or
     normalized order if `sorted`) as `*n` consecutive handles starting at
     `*first`. Returns TLX_OK or TLX_ERR. */
  int (*elems)(tlx_ctx *cx, uint64_t set, uint32_t sorted, uint64_t *first, uint64_t *n);
  /* Value handles live until the generated function returns. `release`
     frees the handles made after `mark` returned its watermark (bounds the
     handles of loop bodies). */
  uint64_t (*mark)(tlx_ctx *cx);
  void (*release)(tlx_ctx *cx, uint64_t mark);
  /* --- v5: value store handles (experimental, TLX_VALUE_STORE) --- */
  /* Run operation `op` of the host's value store operation table (see
     tlx_spec_info.refops_hash) on `args` (handles, integers, Booleans as
     0/1, encodings as pointers, as the operation states); the result goes
     to `out` (one word, or an encoding). Returns TLX_OK or TLX_ERR (the
     host re-runs the whole call in the interpreter). */
  int (*ref_op)(tlx_ctx *cx, uint32_t op, const uint64_t *args, uint64_t *out);
  /* The elements of the stored set `set` as `*n` handles at `*first`
     (immutable while the run lasts). Returns TLX_OK or TLX_ERR. */
  int (*ref_elems)(tlx_ctx *cx, uint64_t set, const uint64_t **first, uint64_t *n);
} tlx_host;

/* The Rust side of the ABI (`native.rs`) keeps `Host`, `HostFn::offset` and
 * `HOST_SIZE`/`HOST_ALIGN` in step with this declaration: the callbacks are
 * the nine fields, in this order, back to back. These assertions fail the C
 * build on a reorder, a gap, a stray field or a different alignment, and
 * `tests/native.rs` compiles this header and compares every offset, size,
 * alignment and prototype with the Rust descriptor. */
_Static_assert(sizeof(tlx_host) == 9 * sizeof(void (*)(void)), "tlx_host is its nine callbacks");
_Static_assert(_Alignof(tlx_host) == _Alignof(void (*)(void)), "tlx_host is aligned like its callbacks");
#define TLX_HOST_AFTER(prev, field) \
  _Static_assert(offsetof(tlx_host, field) == offsetof(tlx_host, prev) + sizeof(void (*)(void)), "tlx_host fields are back to back")
TLX_HOST_AFTER(emit, error);
TLX_HOST_AFTER(error, miss);
TLX_HOST_AFTER(miss, eval);
TLX_HOST_AFTER(eval, elems);
TLX_HOST_AFTER(elems, mark);
TLX_HOST_AFTER(mark, release);
TLX_HOST_AFTER(release, ref_op);
TLX_HOST_AFTER(ref_op, ref_elems);
#undef TLX_HOST_AFTER

typedef struct {
  uint32_t abi_version;
  uint32_t words;
  uint32_t n_actions;
  uint32_t n_invariants;
  const char *const *action_names;
  const char *const *invariant_names;
  uint32_t n_errors;
  const char *const *error_msgs;
  /* v3: number of host regions the code calls and the hash of the host's
     region table it was generated for (both 0 without regions). */
  uint32_t n_regions;
  uint64_t regions_hash;
  /* v5: number of value store operations the code calls and the hash of
     the host's operation table it was generated for (both 0 without). */
  uint32_t n_refops;
  uint64_t refops_hash;
} tlx_spec_info;

const tlx_spec_info *tlx_info(void);
int tlx_init(const tlx_host *h, tlx_ctx *cx);
int tlx_next(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, uint32_t action);
/* 1 = holds, 0 = violated, TLX_ERR = error. */
int tlx_invariant(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, uint32_t inv);
/* Action property `i` on the transition s -> t (optional symbol).
   1 = holds, 0 = violated, negative = error/unsupported. */
int tlx_action_property(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, const uint64_t *t, uint32_t i);

/* Fingerprint reference (64-bit mixing over words): the host's `fp_words`
 * and the JIT's `fp_words_ir` are checked against this compiled by cc. */
static inline uint64_t tlx_mix64(uint64_t x) {
  x ^= x >> 30; x *= 0xbf58476d1ce4e5b9ULL;
  x ^= x >> 27; x *= 0x94d049bb133111ebULL;
  x ^= x >> 31; return x;
}
/* Longer states are hashed in 8 independent lanes (word i in lane i % 8),
   so the mixing chains overlap. */
static inline uint64_t tlx_fp_words(const uint64_t *s, uint32_t n) {
  const uint64_t k = 0x9e3779b97f4a7c15ULL;
  uint64_t h = k ^ n;
  if (n <= 8) {
    for (uint32_t i = 0; i < n; i++) h = tlx_mix64(h ^ s[i]) + k * (i + 1);
    return tlx_mix64(h);
  }
  uint64_t l[8];
  for (uint32_t j = 0; j < 8; j++) l[j] = h + j;
  for (uint32_t i = 0; i < n; i++) l[i & 7] = tlx_mix64(l[i & 7] ^ s[i]) + k * (i + 1);
  for (uint32_t j = 0; j < 8; j++) h = tlx_mix64(h ^ l[j]) + k * (j + 1);
  return tlx_mix64(h);
}
#endif
