//! Recursion-depth guard for the interpreter.
//!
//! TLA+ has no loops: non-termination can only come from an operator or
//! function definition that calls itself forever (e.g. `RecursiveFcn` or a
//! `RECURSIVE` operator without a base case). TLC runs out of JVM stack and
//! reports an error (`EC.STACK_OVERFLOW`); native code would instead
//! segfault. The evaluator therefore counts the nesting of operator/function
//! applications and reports a normal evaluation error once it gets too deep,
//! before the native stack overflows.
//!
//! The limit is much higher than any legitimate spec needs (the corpus
//! recurses at most a few hundred levels deep), so it only fires on runaway
//! recursion. `TLX_MAX_DEPTH` overrides it (tests, fuzzing). The evaluation
//! threads are given a correspondingly large stack (see
//! `tlx_base::stack::EVAL_STACK_SIZE`), but the stack one level takes depends
//! on the recursion kind and the platform (100,000 levels of a recursion
//! through a function application need more than 512 MiB on Linux arm64),
//! and some threads have a small stack: the guard also fails when the
//! thread's stack is nearly used up (`tlx_base::stack::low`), whichever
//! comes first.

use std::cell::Cell;
use std::sync::OnceLock;

use tlx_base::knobs::{self, Env};
use tlx_sema::hir::Span;

use crate::error::{EResult, EvalError};

// (The default nesting of operator/function applications, `TLX_MAX_DEPTH`,
// is declared in `tlx_base::knobs`. TLC's limit is its JVM stack size
// (scripts run it with `-Xss16m`); the default is the same order of
// magnitude.)

/// The message of a recursion-depth error: TLC's text for a
/// `StackOverflowError` while evaluating (an incorrect recursive
/// definition), adapted to tlx.
const MESSAGE: &str = tlx_base::stack::OVERFLOW_MESSAGE;

fn max_depth() -> u32 {
    static LIMIT: OnceLock<u32> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        // (A bad value is reported at startup by the front end.)
        let n = Env::process().num("TLX_MAX_DEPTH").unwrap_or_else(|_| knobs::default_num("TLX_MAX_DEPTH"));
        u32::try_from(n).unwrap_or(u32::MAX)
    })
}

thread_local! {
    /// Current nesting of operator/function applications on this thread.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Whether `e` is the recursion-depth error. It must not be deferred like
/// other errors of an argument (`bind_params`): evaluating the argument
/// again where it is used would hit the limit again, once per nesting level
/// on the way back up, which takes exponential time.
pub(crate) fn is_depth_error(e: &EvalError) -> bool {
    e.message == MESSAGE
}

/// The recursion error at `span` (for recursions the guard does not count).
pub(crate) fn error(span: Span) -> EvalError {
    EvalError::at(MESSAGE, span)
}

/// RAII guard for one operator/function application. Dropping it leaves the
/// nesting, also when the evaluation returns an error.
pub(crate) struct Guard(());

impl Guard {
    /// Enter one application nesting level, or fail with the recursion
    /// error when the limit is reached.
    #[inline]
    pub(crate) fn enter(span: Span) -> EResult<Guard> {
        let max = max_depth();
        DEPTH.with(|d| {
            let n = d.get();
            if n >= max || tlx_base::stack::low() {
                Err(EvalError::at(MESSAGE, span))
            } else {
                d.set(n + 1);
                Ok(Guard(()))
            }
        })
    }
}

impl Drop for Guard {
    #[inline]
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get() - 1));
    }
}
