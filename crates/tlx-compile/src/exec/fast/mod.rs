//! Closure-compiled IR executor (docs/design.md §6.6).
//!
//! Each lowered function (action, predicate, compiled `FnDef`) is compiled
//! once into a tree of boxed closures, specialized by representation:
//! booleans and integers are returned unboxed, encoded values are written
//! into fixed word slots of a per-function frame (no allocation per node),
//! boxed values (`Ty::Val`) are `tlx_value::Value`s. Temps are resolved to
//! frame slots at compile time. Interpreter fallbacks (`Interp`, `ValOp`)
//! call the interpreter.
//!
//! Every IR node is covered. A function that fails to compile (an
//! internal representation mismatch) is treated as unlowered
//! and run by the interpreter.
//!
//! Entry points: actions and Init (`run_action_words`, `run_init_words`),
//! state and transition predicates (`eval_pred_words`, `eval_trans_words`), and
//! expressions over a function's temps (`ExprProg`: mixed-mode host
//! regions, `eval_region`, and the VIEW, `eval_words`).

mod expr;
mod stmt;

use std::cell::RefCell;
use std::sync::Arc;

use tlx_eval::eval::{Ctx, DefCache};
use tlx_eval::{EvalError, Interp};
use tlx_value::Value;

use super::{S0Decoder, StateKeys};
use crate::ir::*;
use crate::types::Typing;

type R<T> = Result<T, EvalError>;

/// Location of an encoded value: a word offset in the current frame, the
/// current state or the next state (the top two bits select the space).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Loc(u32);

const SPACE_SHIFT: u32 = 30;
const OFF_MASK: u32 = (1 << SPACE_SHIFT) - 1;
const SP_FRAME: u32 = 0;
const SP_S0: u32 = 1;
const SP_S1: u32 = 2;
const SP_CONST: u32 = 3;

impl Loc {
    fn frame(off: usize) -> Loc {
        Loc(off as u32)
    }
    fn s0(off: usize) -> Loc {
        Loc((SP_S0 << SPACE_SHIFT) | off as u32)
    }
    fn s1(off: usize) -> Loc {
        Loc((SP_S1 << SPACE_SHIFT) | off as u32)
    }
    fn cnst(off: usize) -> Loc {
        Loc((SP_CONST << SPACE_SHIFT) | off as u32)
    }
    fn off(self) -> usize {
        (self.0 & OFF_MASK) as usize
    }
    fn space(self) -> u32 {
        self.0 >> SPACE_SHIFT
    }
    /// The same space, `k` words further.
    fn plus(self, k: usize) -> Loc {
        Loc(self.0 + k as u32)
    }
}

/// Locals of one activation: encoded temps and scratch slots (`w`),
/// boolean / integer temps (`iv`), boxed temps (`vv`).
#[derive(Default)]
pub(crate) struct Frame {
    w: Vec<u64>,
    iv: Vec<i64>,
    vv: Vec<Option<Value>>,
}

/// Sizes of a function's frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FrameSize {
    words: usize,
    ints: usize,
    vals: usize,
}

impl Frame {
    /// Make room for a frame of size `s` (stale contents are never read:
    /// a temp or slot is written before it is read).
    fn fit(&mut self, s: FrameSize) {
        if self.w.len() < s.words {
            self.w.resize(s.words, 0);
        }
        if self.iv.len() < s.ints {
            self.iv.resize(s.ints, 0);
        }
        if self.vv.len() < s.vals {
            self.vv.resize(s.vals, None);
        }
    }
}

pub(crate) type BoolF = Box<dyn Fn(&mut Fx) -> R<bool> + Send + Sync>;
pub(crate) type IntF = Box<dyn Fn(&mut Fx) -> R<i64> + Send + Sync>;
pub(crate) type EncF = Box<dyn Fn(&mut Fx) -> R<Loc> + Send + Sync>;
pub(crate) type ValF = Box<dyn Fn(&mut Fx) -> R<Value> + Send + Sync>;
/// A statement: runs its continuation, handing complete next states to
/// the emitter; false = stop.
pub(crate) type StmtF = Box<dyn Fn(&mut Fx, &mut dyn FnMut(&mut Fx) -> R<bool>) -> R<bool> + Send + Sync>;

/// A compiled expression, by representation (`Enc` with its word count).
pub(crate) enum Code {
    B(BoolF),
    I(IntF),
    E(EncF, usize),
    V(ValF),
}

/// A compiled `FnDef`.
pub(crate) struct FnProg {
    params: Vec<expr::Slot>,
    body: Code,
    frame: FrameSize,
    consts: Box<[u64]>,
    budgeted: bool,
}

/// A compiled action.
pub struct ActionProg {
    body: StmtF,
    frame: FrameSize,
    consts: Box<[u64]>,
}

/// A compiled predicate.
pub struct PredProg {
    body: BoolF,
    frame: FrameSize,
    consts: Box<[u64]>,
}

/// The compiled functions of a lowering.
pub struct Program {
    funcs: Vec<Option<FnProg>>,
    /// Why each function of the typing is not compiled (`None`: it is).
    /// Cold: built once, when the program is compiled, and read only where
    /// a diagnostic is formed (docs: CODE-24); the evaluation path never
    /// touches it. The table is what a `.ok()` would otherwise erase: the
    /// cause of a function the closure executor could not lower, including
    /// the callee a transitive rejection started at.
    reasons: Vec<Option<NotCoveredReason>>,
}

/// Why a function was not compiled.
pub type NotCovered = String;

/// Why a `FnDef` of a `Program` is not executed by the closure compiler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotCoveredReason {
    /// The closure compiler rejected the function body itself.
    Body(NotCovered),
    /// The function calls one that is not compiled (the executor has no
    /// cross-function fallback): `func` is the callee, `cause` why it is
    /// not compiled.
    Callee { func: u32, cause: Box<NotCoveredReason> },
}

impl NotCoveredReason {
    /// The cause, with the function names of `typing`. Cold path.
    pub fn render(&self, typing: &Typing) -> String {
        match self {
            NotCoveredReason::Body(why) => why.clone(),
            NotCoveredReason::Callee { func, cause } => {
                let name = &typing.funcs[*func as usize].name;
                format!("calls uncompiled function {name}: {}", cause.render(typing))
            }
        }
    }
}

/// Prefix of the rejection a `Compiler` with `callees: None` (a lowered
/// action, predicate or host region, whose calls are resolved against the
/// compiled functions) reports for a callee that is not compiled
/// (`expr::Compiler::call`). `Program::explain` appends the cause the
/// reason table holds; the two must stay in step.
const CALL_OF_UNCOMPILED: &str = "call of uncompiled function ";

/// How many function reasons a diagnostic note names before it counts the
/// rest (a model may have many uncompiled functions; the note stays
/// scannable and the counts still tell the whole story).
const MAX_FUNCTION_REASONS: usize = 8;

/// The rejection fixpoint the compiled backends share (audit F7): a
/// function that calls one a backend cannot translate cannot be translated
/// either, so a rejection reaches every caller transitively.
///
/// `round` is one backend pass over its own candidates: it probes them in
/// the backend's order, records each rejection in the backend's own mask
/// and diagnostics, and returns whether it rejected any. The driver repeats
/// rounds until one rejects none. Only this "iterate to a fixpoint" fact is
/// shared; the probe, the candidate order, the mask, the function ids and
/// the definition pass stay in the backend (CODE-08: share the repeated
/// semantic decision, keep representation-specific loops; ARCH-08: the
/// backends discover nativeness differently, so this is not a generic
/// emission driver).
///
/// The closure backend snapshots its candidates once per round, while the
/// two emitters read their live mask; that choice decides which callee a
/// transitive rejection names, so it stays in each round.
pub(crate) fn rejection_fixpoint(mut round: impl FnMut() -> bool) {
    while round() {}
}

/// Null every function that calls one that is not compiled, recording the
/// transitive cause in `reasons` (the closure executor has no
/// cross-function fallback, so such a call cannot be lowered). Generic in
/// the compiled payload, which the fixpoint never looks at. The round runs
/// through the shared `rejection_fixpoint`; the callee lists and the reason
/// chain are the closure backend's own.
fn close_uncompiled<T>(compiled: &mut [Option<(T, Vec<u32>)>], reasons: &mut [Option<NotCoveredReason>]) {
    rejection_fixpoint(|| {
        // The candidates of this round are judged against one snapshot, so
        // a caller names the callee that was already not compiled when the
        // round began: its cause is set by then (directly when `compile_fn`
        // rejected it, by the round that nulled it otherwise).
        let ok: Vec<bool> = compiled.iter().map(Option::is_some).collect();
        let mut changed = false;
        for (i, c) in compiled.iter_mut().enumerate() {
            let callee = match c.as_ref() {
                Some((_, callees)) => callees.iter().copied().find(|&k| !ok[k as usize]),
                None => None,
            };
            let Some(k) = callee else { continue };
            *c = None;
            if reasons[i].is_none() {
                let cause = reasons[k as usize].clone().unwrap_or(NotCoveredReason::Body("not compiled".into()));
                reasons[i] = Some(NotCoveredReason::Callee { func: k, cause: Box::new(cause) });
            }
            changed = true;
        }
        changed
    });
}

impl Program {
    /// Compile the `FnDef`s of `typing`.
    pub fn new(typing: &Typing) -> Program {
        if let Err(e) = crate::verify::enforce(crate::verify::typing(typing)) {
            let why = NotCoveredReason::Body(format!("typing did not verify: {e}"));
            return Program {
                funcs: (0..typing.funcs.len()).map(|_| None).collect(),
                reasons: (0..typing.funcs.len()).map(|_| Some(why.clone())).collect(),
            };
        }
        Self::verified(typing)
    }

    fn verified(typing: &Typing) -> Program {
        let t = typing;
        let mut compiled: Vec<Option<(FnProg, Vec<u32>)>> = Vec::with_capacity(t.funcs.len());
        let mut reasons: Vec<Option<NotCoveredReason>> = Vec::with_capacity(t.funcs.len());
        for f in &t.funcs {
            match expr::compile_fn(t, f) {
                Ok(c) => {
                    compiled.push(Some(c));
                    reasons.push(None);
                }
                Err(why) => {
                    compiled.push(None);
                    reasons.push(Some(NotCoveredReason::Body(why)));
                }
            }
        }
        close_uncompiled(&mut compiled, &mut reasons);
        Program { funcs: compiled.into_iter().map(|c| c.map(|(f, _)| f)).collect(), reasons }
    }

    /// Why function `func` is not compiled (`None`: it is). Cold path.
    pub fn reason(&self, func: u32) -> Option<&NotCoveredReason> {
        self.reasons.get(func as usize)?.as_ref()
    }

    /// `name: cause` for each uncompiled function of `typing`, at most
    /// `limit` entries followed by a count of the rest, for a diagnostic
    /// note. Cold path.
    pub fn uncovered(&self, typing: &Typing, limit: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut total = 0usize;
        for (i, reason) in self.reasons.iter().enumerate() {
            let Some(reason) = reason else { continue };
            total += 1;
            if out.len() < limit {
                out.push(format!("{}: {}", typing.funcs[i].name, reason.render(typing)));
            }
        }
        if total > out.len() {
            out.push(format!("(+{} more)", total - out.len()));
        }
        out
    }

    /// Append the cause of a callee the compiler could only name
    /// (`CALL_OF_UNCOMPILED`): a lowered action, predicate or host region
    /// calling an uncompiled function must report why that function is not
    /// compiled, not just its name. Other rejections pass through.
    fn explain(&self, typing: &Typing, error: NotCovered) -> NotCovered {
        let Some(name) = error.strip_prefix(CALL_OF_UNCOMPILED) else { return error };
        let Some(func) = typing.funcs.iter().position(|f| &*f.name == name) else { return error };
        match self.reason(func as u32) {
            Some(reason) => format!("{error} ({})", reason.render(typing)),
            None => error,
        }
    }

    pub fn action(&self, typing: &Typing, f: &ActionFn) -> Result<ActionProg, NotCovered> {
        crate::verify::enforce(crate::verify::typing(typing))?;
        crate::verify::enforce(crate::verify::action(typing, f, crate::verify::Context::Action))?;
        self.verified_action(typing, f)
    }

    /// Compile Init, which has no current-state buffer.
    pub fn init(&self, typing: &Typing, f: &ActionFn) -> Result<ActionProg, NotCovered> {
        crate::verify::enforce(crate::verify::typing(typing))?;
        crate::verify::enforce(crate::verify::action(typing, f, crate::verify::Context::Init))?;
        self.verified_action(typing, f)
    }

    fn verified_action(&self, typing: &Typing, f: &ActionFn) -> Result<ActionProg, NotCovered> {
        let mut c = expr::Compiler::new(typing, &f.temps, &self.funcs);
        c.scoped_state = f.body.has_composition();
        let body = c.stmt(&f.body).map_err(|e| self.explain(typing, e))?;
        Ok(ActionProg { body, frame: c.frame_size(), consts: c.take_consts() })
    }

    pub fn predicate(&self, typing: &Typing, p: &PredFn) -> Result<PredProg, NotCovered> {
        crate::verify::enforce(crate::verify::typing(typing))?;
        crate::verify::enforce(crate::verify::predicate(typing, p, crate::verify::Context::State))?;
        self.verified_predicate(typing, p)
    }

    /// Compile an action property, with both complete states available.
    pub fn transition(&self, typing: &Typing, p: &PredFn) -> Result<PredProg, NotCovered> {
        crate::verify::enforce(crate::verify::typing(typing))?;
        crate::verify::enforce(crate::verify::predicate(typing, p, crate::verify::Context::Transition))?;
        self.verified_predicate(typing, p)
    }

    fn verified_predicate(&self, typing: &Typing, p: &PredFn) -> Result<PredProg, NotCovered> {
        let mut c = expr::Compiler::new(typing, &p.temps, &self.funcs);
        c.scoped_state = p.body.has_composition();
        let body = c.bool_(&p.body).map_err(|e| self.explain(typing, e))?;
        Ok(PredProg { body, frame: c.frame_size(), consts: c.take_consts() })
    }
}

/// A compiled expression over the temps of a lowered function, some of
/// which are inputs (mixed-mode host regions, the VIEW).
pub struct ExprProg {
    body: Code,
    inputs: Vec<(expr::Slot, Ty)>,
    frame: FrameSize,
    consts: Box<[u64]>,
}

impl Program {
    /// Compile `e`, an expression of a function with temps `temps`, of
    /// which `inputs` are set by the caller.
    pub fn expr(&self, typing: &Typing, temps: &[Ty], inputs: &[Temp], e: &Expr) -> Result<ExprProg, NotCovered> {
        crate::verify::enforce(crate::verify::typing(typing))?;
        crate::verify::enforce(crate::verify::expression(typing, temps, inputs, e))?;
        let mut c = expr::Compiler::new(typing, temps, &self.funcs);
        c.scoped_state = e.has_composition();
        let body = (|| -> expr::C<Code> {
            Ok(match e.ty {
                Ty::Bool => Code::B(c.bool_(e)?),
                Ty::Int => Code::I(c.int_(e)?),
                Ty::Enc(_) => match c.expr(e)? {
                    code @ Code::E(..) => code,
                    _ => return Err(format!("expected an encoded expression ({:?})", e.ty)),
                },
                Ty::Val => Code::V(c.val(e)?),
            })
        })()
        .map_err(|x| self.explain(typing, x))?;
        let inputs = inputs.iter().map(|t| (c.slots[*t as usize], temps[*t as usize])).collect();
        Ok(ExprProg { body, inputs, frame: c.frame_size(), consts: c.take_consts() })
    }
}

/// `dst.copy_from_slice(src)`, without a call to memcpy for the short
/// copies that make most of them (encodings of a few words).
#[inline(always)]
pub(crate) fn copy_words(dst: &mut [u64], src: &[u64]) {
    macro_rules! fixed {
        ($n:literal) => {{
            let s: [u64; $n] = src[..$n].try_into().expect("length checked");
            dst[..$n].copy_from_slice(&s);
        }};
    }
    match dst.len() {
        0 => {}
        1 => dst[0] = src[0],
        2 => fixed!(2),
        3 => fixed!(3),
        4 => fixed!(4),
        5 => fixed!(5),
        6 => fixed!(6),
        7 => fixed!(7),
        8 => fixed!(8),
        _ => dst.copy_from_slice(src),
    }
}

/// `w.copy_within(from..from + n, to)`, without a call to memmove for
/// short copies.
#[inline(always)]
pub(crate) fn move_words(w: &mut [u64], from: usize, n: usize, to: usize) {
    if from == to {
        return;
    }
    if n <= 4 && (from + n <= to || to + n <= from) {
        let (lo, hi, rev) = if from < to { (from, to, false) } else { (to, from, true) };
        let (a, b) = w.split_at_mut(hi);
        if rev {
            copy_words(&mut a[lo..lo + n], &b[..n]);
        } else {
            copy_words(&mut b[..n], &a[lo..lo + n]);
        }
        return;
    }
    w.copy_within(from..from + n, to);
}

/// Buffers reused across runs on a thread.
#[derive(Default)]
struct Bufs {
    fr: Frame,
    pool: Vec<Frame>,
    s1: Vec<u64>,
    assigned: Vec<bool>,
    s1_dec: Vec<Option<Value>>,
    counts: Vec<u64>,
    spare_s1: Vec<SpareS1>,
}

/// Next-state buffers of `ENABLED` (encoded words, assigned flags,
/// decoded values).
type SpareS1 = (Vec<u64>, Vec<bool>, Vec<Option<Value>>);

thread_local! {
    static BUFS: RefCell<Vec<Bufs>> = const { RefCell::new(Vec::new()) };
    // Composition borrows its parent's temp frame, so keep its partial-state
    // buffers separate from ordinary function frames.
    static COMPOSITION_BUFS: RefCell<Vec<Bufs>> = const { RefCell::new(Vec::new()) };
}

/// The program a compiled function belongs to, and how it runs.
#[derive(Clone, Copy)]
pub struct Run<'a> {
    pub ip: &'a Arc<Interp>,
    pub typing: &'a Typing,
    pub prog: &'a Program,
    /// `TLCGet("level")`.
    pub level: u32,
    /// Decoder of the encoded current state (default: the layout's).
    pub decoder: Option<&'a S0Decoder<'a>>,
    /// Memo scope of pure interpreter fallbacks (0: off; `exec::memo`).
    pub memo: u64,
}

impl<'a> Run<'a> {
    /// A run over encoded states of `typing`.
    pub fn words(ip: &'a Arc<Interp>, typing: &'a Typing, prog: &'a Program, level: u32) -> Run<'a> {
        Run { ip, typing, prog, level, decoder: None, memo: 0 }
    }

    pub fn with_decoder(mut self, d: &'a S0Decoder<'a>) -> Run<'a> {
        self.decoder = Some(d);
        self
    }

    /// Memoize pure interpreter fallbacks in scope `id` (0: off; see
    /// `memo::MemoScope::exec_id`).
    pub fn with_memo(mut self, id: u64) -> Run<'a> {
        self.memo = id;
        self
    }
}

/// The execution context of a compiled function.
pub struct Fx<'a> {
    ip: &'a Arc<Interp>,
    t: &'a Typing,
    funcs: &'a [Option<FnProg>],
    /// Encoded current state (empty in Init).
    s0: &'a [u64],
    /// No current state (Init): state variables are read from the next
    /// state.
    init: bool,
    /// Encoded constants of the running function.
    consts: &'a [u64],
    /// Current state values (decoded on demand).
    s0_vals: Option<Arc<[Value]>>,
    decoder: Option<&'a S0Decoder<'a>>,
    /// Encoded next state, unless borrowed (`s1_ext`).
    s1: Vec<u64>,
    /// A borrowed, read-only encoded next state (a complete one, or the C
    /// code's `s1` in a mixed-mode region); None: `s1`.
    s1_ext: Option<&'a [u64]>,
    assigned: Vec<bool>,
    /// The decoded assigned next-state variables (None: not decoded yet).
    s1_dec: Vec<Option<Value>>,
    /// The next-state variables interpreter fallbacks may read (only those
    /// are decoded); None: all assigned ones.
    s1_reads: Option<&'a [u32]>,
    /// A next state is in scope (actions, transition predicates).
    has_s1: bool,
    fr: Frame,
    pool: Vec<Frame>,
    counts: Vec<u64>,
    /// Unused next-state buffers of `ENABLED`.
    spare_s1: Vec<SpareS1>,
    level: u32,
    depth: u32,
    calls: u64,
    /// Memo scope of pure interpreter fallbacks (0: off).
    memo: u64,
    composed: u64,
    /// The definition cache of the interpreter fallbacks (made on first
    /// use; `cache_primed`: it may hold next-state values, the next state
    /// being complete).
    cache: Option<DefCache<'a>>,
    cache_primed: bool,
}

impl<'a> Fx<'a> {
    fn new(r: &Run<'a>, frame: FrameSize, consts: &'a [u64], has_s1: bool) -> Fx<'a> {
        let b = BUFS.with(|b| b.borrow_mut().pop()).unwrap_or_default();
        let Bufs { mut fr, pool, mut s1, mut assigned, mut s1_dec, mut counts, spare_s1 } = b;
        fr.fit(frame);
        let nvars = r.ip.prog.vars.len();
        // (Every assignment is undone at the end of a run: `assigned` is
        // all false and `s1_dec` all None again.)
        let words = r.typing.layout.words;
        if s1.len() != words {
            s1.clear();
            s1.resize(words, 0);
        }
        if assigned.len() != nvars {
            assigned.clear();
            assigned.resize(nvars, false);
        }
        if s1_dec.len() != nvars {
            s1_dec.clear();
            s1_dec.resize(nvars, None);
        }
        counts.clear();
        Fx {
            ip: r.ip,
            t: r.typing,
            funcs: &r.prog.funcs,
            s0: &[],
            init: false,
            consts,
            s0_vals: None,
            decoder: r.decoder,
            s1,
            s1_ext: None,
            assigned,
            s1_dec,
            s1_reads: None,
            has_s1,
            fr,
            pool,
            counts,
            spare_s1,
            level: r.level,
            depth: 0,
            calls: 0,
            memo: r.memo,
            composed: 0,
            cache: None,
            cache_primed: false,
        }
    }

    /// A private action frame over another current state. Captured temps
    /// retain their values; definition caches and the partial next state do not.
    /// Temporaries and scratch slots are unique across the statement tree,
    /// so the child can borrow the frame buffer and return it after execution.
    fn composition_frame<'b>(&'b mut self, s0: &'b [u64], same_current: bool) -> Fx<'b> {
        let b = COMPOSITION_BUFS.with(|b| b.borrow_mut().pop()).unwrap_or_default();
        let Bufs { pool, mut s1, mut assigned, mut s1_dec, mut counts, spare_s1, .. } = b;
        s1.resize(self.t.layout.words, 0);
        assigned.clear();
        assigned.resize(self.assigned.len(), false);
        s1_dec.clear();
        s1_dec.resize(self.assigned.len(), None);
        counts.clear();
        counts.extend_from_slice(&self.counts);
        Fx {
            ip: self.ip,
            t: self.t,
            funcs: self.funcs,
            s0,
            init: false,
            consts: self.consts,
            // The first operand uses the parent's immutable current state.
            // Reuse its decoder without sharing any next-state/definition cache;
            // the second operand must decode its own intermediate state.
            s0_vals: if same_current { self.s0_vals.clone() } else { None },
            decoder: if same_current { self.decoder } else { None },
            s1,
            s1_ext: None,
            assigned,
            s1_dec,
            s1_reads: None,
            has_s1: true,
            fr: std::mem::take(&mut self.fr),
            pool,
            counts,
            spare_s1,
            level: self.level,
            depth: self.depth,
            calls: self.calls,
            memo: 0,
            composed: 0,
            cache: None,
            cache_primed: false,
        }
    }

    fn finish_composition(self) -> Frame {
        let Fx { fr, pool, s1, assigned, s1_dec, counts, spare_s1, .. } = self;
        COMPOSITION_BUFS.with(|b| {
            b.borrow_mut().push(Bufs { fr: Frame::default(), pool, s1, assigned, s1_dec, counts, spare_s1 });
        });
        fr
    }

    fn release(self) {
        let Fx { fr, pool, s1, assigned, s1_dec, counts, spare_s1, .. } = self;
        BUFS.with(|b| b.borrow_mut().push(Bufs { fr, pool, s1, assigned, s1_dec, counts, spare_s1 }));
    }

    fn typing(&self) -> &'a Typing {
        self.t
    }

    /// The encoded next state.
    #[inline]
    fn s1w(&self) -> &[u64] {
        match self.s1_ext {
            Some(w) => w,
            None => &self.s1,
        }
    }

    /// The words at `l` (`n` of them).
    #[inline]
    fn get(&self, l: Loc, n: usize) -> &[u64] {
        let o = l.off();
        match l.space() {
            SP_FRAME => &self.fr.w[o..o + n],
            SP_S0 => &self.s0[o..o + n],
            SP_S1 => &self.s1w()[o..o + n],
            _ => &self.consts[o..o + n],
        }
    }

    /// Word `i` at `l`.
    #[inline]
    fn word(&self, l: Loc, i: usize) -> u64 {
        let o = l.off() + i;
        match l.space() {
            SP_FRAME => self.fr.w[o],
            SP_S0 => self.s0[o],
            SP_S1 => self.s1w()[o],
            _ => self.consts[o],
        }
    }

    /// Copy the `n` words at `src` to frame offset `dst`.
    #[inline]
    fn copy_to(&mut self, src: Loc, dst: usize, n: usize) {
        let o = src.off();
        match src.space() {
            SP_FRAME => move_words(&mut self.fr.w, o, n, dst),
            SP_S0 => copy_words(&mut self.fr.w[dst..dst + n], &self.s0[o..o + n]),
            SP_S1 => {
                let s1 = match self.s1_ext {
                    Some(w) => w,
                    None => &self.s1[..],
                };
                copy_words(&mut self.fr.w[dst..dst + n], &s1[o..o + n])
            }
            _ => copy_words(&mut self.fr.w[dst..dst + n], &self.consts[o..o + n]),
        }
    }

    /// Copy the `n` words at `src` to offset `dst` of the (owned) next
    /// state.
    #[inline]
    fn copy_to_s1(&mut self, src: Loc, dst: usize, n: usize) {
        debug_assert!(self.s1_ext.is_none(), "assignment to a borrowed next state");
        let o = src.off();
        match src.space() {
            SP_FRAME => copy_words(&mut self.s1[dst..dst + n], &self.fr.w[o..o + n]),
            SP_S0 => copy_words(&mut self.s1[dst..dst + n], &self.s0[o..o + n]),
            SP_S1 => move_words(&mut self.s1, o, n, dst),
            _ => copy_words(&mut self.s1[dst..dst + n], &self.consts[o..o + n]),
        }
    }

    /// The current state's values.
    fn s0_values(&mut self) -> R<&[Value]> {
        if self.init {
            return Err(EvalError::new("internal: current state read in Init"));
        }
        if self.s0_vals.is_none() {
            let vals = match self.decoder {
                Some(d) => d(self.s0),
                // SAFETY: valid state words with published Dyn handles.
                None => unsafe { self.typing().layout.decode_state_unchecked(self.s0) }.into(),
            };
            self.s0_vals = Some(vals);
        }
        Ok(self.s0_vals.as_deref().expect("decoded above"))
    }

    /// Decode the assigned next-state variables not decoded yet (encoded
    /// mode; only those in `s1_reads` if given).
    fn decode_s1(&mut self) {
        let t = self.t;
        let w = match self.s1_ext {
            Some(w) => w,
            None => &self.s1[..],
        };
        let (assigned, dec) = (&self.assigned, &mut self.s1_dec);
        let mut one = |i: usize| {
            if assigned[i] && dec[i].is_none() {
                dec[i] = Some(
                    // SAFETY: valid words from the matching encoder/program, with published Dyn handles.
                    unsafe { t.layout.node(i).decode_unchecked(&w[t.var_range(i)]) },
                );
            }
        };
        match self.s1_reads {
            Some(vs) => vs.iter().for_each(|&i| one(i as usize)),
            None => (0..assigned.len()).for_each(one),
        }
    }

    /// Whether every next-state variable is assigned (primed definition
    /// values may then be cached: the next state stays fixed).
    fn s1_complete(&self) -> bool {
        self.has_s1 && self.assigned.iter().all(|a| *a)
    }

    /// Run `f` with an interpreter context for the current and next state.
    fn with_ctx<T>(&mut self, f: impl FnOnce(&Arc<Interp>, &Ctx) -> R<T>) -> R<T> {
        if self.has_s1 {
            self.decode_s1();
        }
        if self.init {
            return f(self.ip, &Ctx::init(&self.s1_dec));
        }
        self.s0_values()?;
        let s0: &[Value] = self.s0_vals.as_deref().expect("decoded above");
        if self.cache.is_none() {
            self.cache = Some(self.new_cache());
        }
        let cache = self.cache.as_ref().expect("made above");
        let cx = if self.has_s1 {
            Ctx::action(s0, &self.s1_dec, self.level).with_cache(cache)
        } else {
            Ctx::state(s0, self.level).with_cache(cache)
        };
        f(self.ip, &cx)
    }

    /// The definition cache of the interpreter fallbacks.
    fn cache(&mut self) -> &mut DefCache<'a> {
        if self.cache.is_none() {
            self.cache = Some(self.new_cache());
        }
        self.cache.as_mut().expect("made above")
    }

    /// A definition cache for this run. On encoded states it shares the
    /// memos of the current and (if complete) next state with the other
    /// runs on the same states (`def_memo`): the interpreter fallbacks of
    /// every region, predicate and action property evaluated on a state
    /// or a step compute each state-level definition once.
    fn new_cache(&self) -> DefCache<'a> {
        if !self.init {
            // (A complete next state is borrowed: transition predicates
            // and regions.)
            let s1 = self.s1_ext.filter(|_| self.cache_primed);
            DefCache::from_source(Box::new(StateKeys { ip: self.ip, layout: &self.t.layout, s0: self.s0, s1 }), self.cache_primed)
        } else if self.cache_primed {
            DefCache::default()
        } else {
            DefCache::unprimed()
        }
    }

    /// Undo the next state's assignments of a run that set them as a whole
    /// (transition predicates, regions).
    fn clear_s1(&mut self) {
        self.assigned.fill(false);
        self.s1_dec.fill(None);
    }
}

/// Run a compiled action on an encoded state, handing each successor to
/// `emit` (false: stop). An error means the caller must evaluate the
/// action otherwise.
/// # Safety
/// Input words (states and region arguments) must have the program's encodings;
/// all active Dyn handles must satisfy `tlx_layout::dynval::lookup_unchecked`'s
/// valid-handle and publication contract. The typing and program must match.
pub unsafe fn run_action_words(r: &Run<'_>, a: &ActionProg, s0: &[u64], emit: &mut dyn FnMut(&[u64]) -> R<bool>) -> R<bool> {
    let mut fx = Fx::new(r, a.frame, &a.consts, true);
    fx.s0 = s0;
    let mut out = |fx: &mut Fx| emit(fx.s1w());
    let res = (a.body)(&mut fx, &mut out);
    if res.is_ok() {
        fx.ip.count_composed_states(fx.composed);
    }
    fx.release();
    res
}

/// Run compiled Init (encoded), handing each initial state to `emit`.
pub fn run_init_words(r: &Run<'_>, a: &ActionProg, emit: &mut dyn FnMut(&[u64]) -> R<bool>) -> R<bool> {
    let mut fx = Fx::new(r, a.frame, &a.consts, true);
    fx.init = true;
    let mut out = |fx: &mut Fx| emit(fx.s1w());
    let res = (a.body)(&mut fx, &mut out);
    fx.release();
    res
}

/// Evaluate a compiled state predicate on an encoded state.
/// # Safety
/// Input words (states and region arguments) must have the program's encodings;
/// all active Dyn handles must satisfy `tlx_layout::dynval::lookup_unchecked`'s
/// valid-handle and publication contract. The typing and program must match.
pub unsafe fn eval_pred_words(r: &Run<'_>, p: &PredProg, s0: &[u64]) -> R<bool> {
    let mut fx = Fx::new(r, p.frame, &p.consts, false);
    fx.s0 = s0;
    let res = (p.body)(&mut fx);
    fx.release();
    res
}

/// Evaluate a compiled transition predicate (action property) on the
/// encoded step `s0` -> `s1`.
/// # Safety
/// Input words (states and region arguments) must have the program's encodings;
/// all active Dyn handles must satisfy `tlx_layout::dynval::lookup_unchecked`'s
/// valid-handle and publication contract. The typing and program must match.
pub unsafe fn eval_trans_words(r: &Run<'_>, p: &PredProg, s0: &[u64], s1: &[u64]) -> R<bool> {
    let mut fx = Fx::new(r, p.frame, &p.consts, true);
    fx.s0 = s0;
    fx.s1_ext = Some(s1);
    fx.assigned.fill(true);
    // The next state is complete: primed evaluations may be memoized too.
    fx.cache_primed = true;
    let res = (p.body)(&mut fx);
    fx.clear_s1();
    fx.release();
    res
}

/// Evaluate a compiled expression of encoded type on the encoded state
/// `s0` (a state-level context; no inputs), appending its words to `out`.
/// # Safety
/// Input words (states and region arguments) must have the program's encodings;
/// all active Dyn handles must satisfy `tlx_layout::dynval::lookup_unchecked`'s
/// valid-handle and publication contract. The typing and program must match.
pub unsafe fn eval_words(r: &Run<'_>, p: &ExprProg, s0: &[u64], out: &mut Vec<u64>) -> R<()> {
    let mut fx = Fx::new(r, p.frame, &p.consts, false);
    fx.s0 = s0;
    let res = match &p.body {
        Code::E(f, n) => f(&mut fx).map(|l| out.extend_from_slice(fx.get(l, *n))),
        _ => Err(EvalError::new("internal: not an encoded expression")),
    };
    fx.release();
    res
}

/// Evaluate a mixed-mode host region (docs/mixed-mode.md §5): `s0` is
/// the current state (None in Init), `s1` the next state in scope (None
/// in state predicates), of which interpreter fallbacks read the
/// variables `s1_reads` (None: all assigned ones); `args` are the
/// region's inputs, in the order of the inputs it was compiled with.
/// # Safety
/// Input words (states and region arguments) must have the program's encodings;
/// all active Dyn handles must satisfy `tlx_layout::dynval::lookup_unchecked`'s
/// valid-handle and publication contract. The typing and program must match.
pub unsafe fn eval_region<'a>(
    r: &Run<'a>,
    p: &'a ExprProg,
    s0: Option<&'a [u64]>,
    s1: Option<super::S1View<'a>>,
    s1_reads: Option<&'a [u32]>,
    args: &tlx_native::mixed::Args<'_>,
) -> R<tlx_native::mixed::Out> {
    use tlx_native::mixed::{In, Out};
    let mut fx = Fx::new(r, p.frame, &p.consts, s1.is_some());
    match s0 {
        Some(w) => fx.s0 = w,
        None => fx.init = true,
    }
    fx.s1_reads = s1_reads;
    if let Some(v) = s1 {
        fx.s1_ext = Some(v.words);
        fx.assigned.copy_from_slice(v.assigned);
        if v.assigned.iter().all(|a| *a) {
            // A complete next state: primed evaluations may be memoized.
            fx.cache_primed = true;
        }
    }
    for (i, (slot, ty)) in p.inputs.iter().enumerate() {
        match (*slot, *ty, args.get(i)) {
            (expr::Slot::I(k), Ty::Bool, In::Int(x)) => fx.fr.iv[k] = (x != 0) as i64,
            (expr::Slot::I(k), Ty::Int, In::Int(x)) => fx.fr.iv[k] = x,
            (expr::Slot::W(off, n), Ty::Enc(_), In::Words(w)) if w.len() == n => fx.fr.w[off..off + n].copy_from_slice(w),
            (expr::Slot::V(k), Ty::Val, In::Val(v)) => fx.fr.vv[k] = Some(v.clone()),
            (slot, ty, x) => panic!("region input {i} is {x:?}, expected {ty:?} ({slot:?})"),
        }
    }
    let res = match &p.body {
        Code::B(f) => f(&mut fx).map(Out::Bool),
        Code::I(f) => f(&mut fx).map(Out::Int),
        Code::E(f, n) => f(&mut fx).map(|l| Out::Words(fx.get(l, *n).into())),
        Code::V(f) => f(&mut fx).map(Out::Val),
    };
    if s1.is_some() {
        fx.clear_s1();
    }
    fx.release();
    res
}

/// The compiled Init, actions and predicates of a model (None: not
/// covered).
pub struct Compiled {
    pub prog: Program,
    pub init: Option<ActionProg>,
    pub actions: Vec<Option<ActionProg>>,
    pub preds: Vec<Option<PredProg>>,
    /// Transition predicates (action properties, ...).
    pub aprops: Vec<Option<PredProg>>,
}

impl Compiled {
    /// Compile `init`, `actions` and the predicates; a note on what was
    /// not covered (and why) is added to `notes`.
    pub fn new(
        typing: &Typing,
        init: Option<&ActionFn>,
        actions: &[Option<ActionFn>],
        preds: &[PredFn],
        aprops: &[PredFn],
        notes: &mut Vec<String>,
    ) -> Compiled {
        match crate::verify::enforce(crate::verify::Program::new(typing, init, actions, preds, aprops)) {
            Ok(p) => Self::from_verified(&p, notes),
            Err(e) => {
                notes.push(format!("IR verification failed; interpreting model: {e}"));
                Self::empty(actions.len(), preds.len(), aprops.len())
            }
        }
    }

    pub(crate) fn empty(actions: usize, preds: usize, aprops: usize) -> Self {
        Self {
            prog: Program { funcs: Vec::new(), reasons: Vec::new() },
            init: None,
            actions: (0..actions).map(|_| None).collect(),
            preds: (0..preds).map(|_| None).collect(),
            aprops: (0..aprops).map(|_| None).collect(),
        }
    }

    pub fn from_verified(p: &crate::verify::Program<'_>, notes: &mut Vec<String>) -> Self {
        let (typing, init, actions, preds, aprops) = (p.typing(), p.init(), p.actions(), p.predicates(), p.transitions());
        let prog = Program::verified(typing);

        let lowered = actions.iter().filter(|a| a.is_some()).count();
        let mut why: Vec<String> = Vec::new();
        let init = init.and_then(|a| prog.verified_action(typing, a).map_err(|e| why.push(format!("{}: {e}", a.name))).ok());
        let actions: Vec<Option<ActionProg>> = actions
            .iter()
            .map(|a| {
                let a = a.as_ref()?;
                prog.verified_action(typing, a).map_err(|e| why.push(format!("{}: {e}", a.name))).ok()
            })
            .collect();
        let preds: Vec<Option<PredProg>> =
            preds.iter().map(|p| prog.verified_predicate(typing, p).map_err(|e| why.push(format!("{}: {e}", p.name))).ok()).collect();
        let aprops: Vec<Option<PredProg>> =
            aprops.iter().map(|p| prog.verified_predicate(typing, p).map_err(|e| why.push(format!("{}: {e}", p.name))).ok()).collect();
        let fns = prog.funcs.iter().filter(|f| f.is_some()).count();
        if !why.is_empty() || fns < prog.funcs.len() {
            // The functions the closure compiler could not lower, with the
            // cause `Program::verified` retained (a caller of an
            // uncompiled function is one of them, and names its callee's
            // cause).
            why.extend(prog.uncovered(typing, MAX_FUNCTION_REASONS));
            notes.push(format!(
                "IR closures: {}/{} lowered actions, {}/{} predicates, {}/{} transition predicates, {}/{} functions compiled{}",
                actions.iter().filter(|a| a.is_some()).count(),
                lowered,
                preds.iter().filter(|p| p.is_some()).count(),
                preds.len(),
                aprops.iter().filter(|p| p.is_some()).count(),
                aprops.len(),
                fns,
                prog.funcs.len(),
                if why.is_empty() { String::new() } else { format!(" (not: {})", why.join("; ")) }
            ));
        }
        Compiled { prog, init, actions, preds, aprops }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tlx_layout::{Layout, Shape};

    /// The rejection the closure compiler reports for the fixture below:
    /// `Filter` compiles through the encoded path, which requires an encoded
    /// set type, and a boxed `Val` is not one.
    const REJECTED: &str = "expected an encoded type, got Val";

    /// A typing with one state variable, enough for the `Val`-typed `Var` the
    /// fixture reads (the verifier does not shape-check a `Val` state read).
    fn typing() -> Typing {
        Typing::new(Arc::new(Layout::new(vec![Shape::Int { lo: 0, hi: 10 }])))
    }

    /// A `FnDef` the common verifier accepts — a `Filter` over a `Val` set
    /// binds its element as `Val` (`verify::source`) — but the closure
    /// compiler rejects. This is the cause a `.ok()` at the lowering boundary
    /// used to erase.
    fn rejected_fn(name: &str) -> FnDef {
        FnDef {
            name: name.into(),
            params: Vec::new(),
            temps: vec![Ty::Val],
            ret: Ty::Val,
            body: Expr::new(
                Ty::Val,
                ExprKind::Filter {
                    bind: 0,
                    set: Expr::new(Ty::Val, ExprKind::Var(0)),
                    pred: Expr::new(Ty::Bool, ExprKind::Const(Value::TRUE)),
                },
            ),
            budgeted: false,
        }
    }

    fn with_funcs(mut t: Typing, funcs: Vec<FnDef>) -> Typing {
        t.funcs = funcs;
        t.funcs_changed();
        t
    }

    /// A function the closure compiler rejects keeps its cause: a `.ok()` at
    /// the lowering boundary must not erase why it is not covered.
    #[test]
    fn direct_rejection_keeps_the_cause() {
        let t = with_funcs(typing(), vec![rejected_fn("rejected")]);
        let p = Program::new(&t);
        assert!(p.funcs[0].is_none(), "the rejected function is not compiled");
        assert_eq!(p.reason(0), Some(&NotCoveredReason::Body(REJECTED.into())));
        assert_eq!(p.uncovered(&t, 8), vec![format!("rejected: {REJECTED}")]);
    }

    /// A function rejected only because it calls an uncompiled one records the
    /// callee and its cause, transitively, not just "not compiled".
    #[test]
    fn transitive_rejection_names_the_callee_and_its_cause() {
        let caller = FnDef {
            name: "caller".into(),
            params: Vec::new(),
            temps: Vec::new(),
            ret: Ty::Val,
            body: Expr::new(Ty::Val, ExprKind::Call { func: 1, args: Vec::new() }),
            budgeted: false,
        };
        let t = with_funcs(typing(), vec![caller, rejected_fn("rejected")]);
        let p = Program::new(&t);
        assert!(p.funcs[0].is_none(), "a caller of an uncompiled function is not compiled");
        assert_eq!(p.reason(0), Some(&NotCoveredReason::Callee { func: 1, cause: Box::new(NotCoveredReason::Body(REJECTED.into())) }));
        assert_eq!(
            p.uncovered(&t, 8),
            vec![format!("caller: calls uncompiled function rejected: {REJECTED}"), format!("rejected: {REJECTED}"),]
        );
    }

    /// The diagnostic boundary appends the cause of a callee the compiler
    /// could only name (a host region, a lowered expression).
    #[test]
    fn call_of_an_uncompiled_function_gains_its_cause() {
        let t = with_funcs(typing(), vec![rejected_fn("rejected")]);
        let p = Program::new(&t);
        let e = Expr::new(Ty::Val, ExprKind::Call { func: 0, args: Vec::new() });
        assert_eq!(p.expr(&t, &[], &[], &e).err().unwrap(), format!("call of uncompiled function rejected ({REJECTED})"));
    }

    /// Nothing about a fully compiled typing changes: no reason, no note, so
    /// the hot path is untouched.
    #[test]
    fn compiled_functions_carry_no_reason_and_emit_no_note() {
        let t = Typing::new(Arc::new(Layout::new(vec![Shape::Int { lo: 0, hi: 10 }])));
        let id = FnDef {
            name: "id".into(),
            params: vec![0],
            temps: vec![Ty::Int],
            ret: Ty::Int,
            body: Expr::new(Ty::Int, ExprKind::Temp(0)),
            budgeted: false,
        };
        let t = with_funcs(t, vec![id]);
        let p = Program::new(&t);
        assert!(p.funcs[0].is_some());
        assert_eq!(p.reason(0), None);
        assert!(p.uncovered(&t, 8).is_empty());
        let v = crate::verify::Program::new(&t, None, &[], &[], &[]).unwrap();
        let mut notes = Vec::new();
        let c = Compiled::from_verified(&v, &mut notes);
        assert!(c.prog.funcs[0].is_some());
        assert!(notes.is_empty(), "{notes:?}");
    }

    /// The note keeps the coverage counts and names the causes (bounded).
    #[test]
    fn note_reports_counts_and_names_the_causes() {
        let funcs: Vec<FnDef> = (0..10).map(|i| rejected_fn(&format!("f{i}"))).collect();
        let t = with_funcs(typing(), funcs);
        let v = crate::verify::Program::new(&t, None, &[], &[], &[]).unwrap();
        let mut notes = Vec::new();
        assert_eq!(Compiled::from_verified(&v, &mut notes).prog.uncovered(&t, 8).len(), 9);
        let note = notes.join("\n");
        assert!(note.contains("0/10 functions compiled"), "{note}");
        assert!(note.contains(&format!("f0: {REJECTED}")), "{note}");
        assert!(note.contains("(+2 more)"), "{note}");
    }

    /// A `Val`-returning function whose body is calls (`Compile` records
    /// the callees); it is rejected only when a callee is.
    fn caller(name: &str, params: Vec<u32>, temps: Vec<Ty>, body: Expr) -> FnDef {
        FnDef { name: name.into(), params, temps, ret: Ty::Val, body, budgeted: false }
    }

    /// `func(args)` at `Ty::Val` (the fixture functions return `Val`).
    fn call(func: u32, args: Vec<Expr>) -> Expr {
        Expr::new(Ty::Val, ExprKind::Call { func, args })
    }

    /// The shared fixpoint repeats a round until one rejects nothing: the
    /// first round rejects a direct callee, the next its caller, and the
    /// stable round stops the driver.
    #[test]
    fn rejection_fixpoint_repeats_until_a_round_rejects_nothing() {
        let mut native = [true, true, false, true]; // 2 is rejected directly
        let mut rounds = 0;
        rejection_fixpoint(|| {
            rounds += 1;
            let before = native;
            let mut changed = false;
            // 1 calls 2, 3 calls 1: the rejection has to reach 3 through 1.
            for (i, callee) in [(1usize, 2usize), (3, 1)] {
                if native[i] && !before[callee] {
                    native[i] = false;
                    changed = true;
                }
            }
            changed
        });
        assert_eq!(rounds, 3, "reject 1, reject 3, then a stable round");
        assert_eq!(native, [true, false, false, false]);
    }

    /// A cycle of callers does not keep a member native: the rejection
    /// reaches the caller, and the round that would re-reject the (already
    /// rejected) origin is stable.
    #[test]
    fn rejection_fixpoint_closes_a_call_cycle() {
        let mut native = [true, false]; // 1 is rejected directly
        rejection_fixpoint(|| {
            let before = native;
            let mut changed = false;
            for (i, callee) in [(0usize, 1usize), (1, 0)] {
                if native[i] && !before[callee] {
                    native[i] = false;
                    changed = true;
                }
            }
            changed
        });
        assert_eq!(native, [false, false]);
    }

    /// A cycle of compiled functions is left alone: no member is rejected,
    /// so the fixpoint terminates after one stable round.
    #[test]
    fn a_call_cycle_without_a_rejection_stays_compiled() {
        let t = with_funcs(typing(), vec![caller("a", vec![], vec![], call(1, vec![])), caller("b", vec![], vec![], call(0, vec![]))]);
        let p = Program::new(&t);
        assert!(p.funcs[0].is_some() && p.funcs[1].is_some(), "mutual recursion is native");
        assert_eq!(p.reason(0), None);
        assert_eq!(p.reason(1), None);
    }

    /// A rejection inside a call cycle reaches every member: `a` calls `b`,
    /// `b` calls `a` and the rejected `r`, so both callers are rejected.
    #[test]
    fn a_rejection_inside_a_call_cycle_reaches_every_member() {
        let a = caller("a", vec![0], vec![Ty::Val], call(1, vec![]));
        let b = caller("b", vec![], vec![], call(0, vec![call(2, vec![])]));
        let t = with_funcs(typing(), vec![a, b, rejected_fn("r")]);
        let p = Program::new(&t);
        assert!(p.funcs[0].is_none() && p.funcs[1].is_none());
        assert_eq!(
            p.reason(0),
            Some(&NotCoveredReason::Callee {
                func: 1,
                cause: Box::new(NotCoveredReason::Callee { func: 2, cause: Box::new(NotCoveredReason::Body(REJECTED.into())) }),
            })
        );
    }

    /// A longer call chain names each callee on the way to the cause: the
    /// reason nests once per hop and ends at the rejected function.
    #[test]
    fn a_transitive_rejection_chain_names_every_callee() {
        let funcs = vec![
            caller("c0", vec![], vec![], call(1, vec![])),
            caller("c1", vec![], vec![], call(2, vec![])),
            caller("c2", vec![], vec![], call(3, vec![])),
            rejected_fn("r3"),
        ];
        let t = with_funcs(typing(), funcs);
        let p = Program::new(&t);
        assert_eq!(
            p.reason(0),
            Some(&NotCoveredReason::Callee {
                func: 1,
                cause: Box::new(NotCoveredReason::Callee {
                    func: 2,
                    cause: Box::new(NotCoveredReason::Callee { func: 3, cause: Box::new(NotCoveredReason::Body(REJECTED.into())) }),
                }),
            })
        );
        let names = p.uncovered(&t, 8);
        assert_eq!(names.len(), 4, "the three callers and the rejected leaf are named: {names:?}");
        assert!(names[0].starts_with("c0: "), "{names:?}");
        assert!(names[0].contains(&format!("r3: {REJECTED}")), "{names:?}");
    }

    /// The callee a transitive rejection names is the one that was already
    /// not compiled when the round began, not one rejected earlier in the
    /// same round (the closure snapshots its candidates). Here `i` calls
    /// `c` (rejected in this round) and `r` (rejected before it): the
    /// reason is `r`, and `c` is not silently preferred.
    #[test]
    fn reason_names_the_callee_that_was_not_compiled_at_round_start() {
        let c = caller("c", vec![0], vec![Ty::Val], call(0, vec![]));
        let i = caller("i", vec![], vec![], call(1, vec![call(2, vec![])]));
        let t = with_funcs(typing(), vec![rejected_fn("r0"), c, rejected_fn("r2"), i]);
        let p = Program::new(&t);
        assert!(p.funcs[3].is_none(), "the caller of two rejected functions is rejected");
        assert_eq!(p.reason(3), Some(&NotCoveredReason::Callee { func: 2, cause: Box::new(NotCoveredReason::Body(REJECTED.into())) }));
        assert_eq!(p.reason(1), Some(&NotCoveredReason::Callee { func: 0, cause: Box::new(NotCoveredReason::Body(REJECTED.into())) }));
    }
}
