//! Memo inputs must be evaluated in order, but decoded only when needed.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;
use crate::exec::fast::{FrameSize, Loc, Program, Run};
use tlx_eval::EvalError;
use tlx_layout::{Layout, Shape};
use tlx_sema::hir::DefKind;

thread_local! {
    pub(super) static DECODES: Cell<usize> = const { Cell::new(0) };
}

const SPEC: &str = r#"---- MODULE MemoInputs ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = x
Pick(a, b, c) == <<a, b, c>>
====
"#;

fn setup() -> (Arc<tlx_eval::Interp>, crate::types::Typing) {
    let ip = tlx_testkit::spec::interpreter("MemoInputs", SPEC, "INIT Init\nNEXT Next\n");
    let typing = crate::types::Typing::new(Arc::new(Layout::new(vec![Shape::Int { lo: 0, hi: 10 }])));
    DECODES.with(|n| n.set(0));
    (ip, typing)
}

fn node(ip: &tlx_eval::Interp, codes: Vec<(Code, Ty)>) -> InterpNode {
    let def = ip.prog.defs.iter().find(|d| &*d.name == "Pick").unwrap();
    let DefKind::Op(expr) = &def.kind else { panic!("operator") };
    InterpNode {
        node: 0x1234,
        expr: expr.clone(),
        binds: def.params.iter().map(|&local| (local, Expr::new(Ty::Val, ExprKind::Const(Value::TRUE)))).collect(),
        codes: def.params.iter().zip(codes).map(|(&local, (code, ty))| InterpBind { local, def: None, code, ty }).collect(),
        deferred: vec![],
        env: Env::default(),
    }
}

/// Each input writes the same scratch word, as a later evaluation may do.
fn encoded(value: u64, calls: &Arc<AtomicUsize>) -> (Code, Ty) {
    let calls = calls.clone();
    (
        Code::E(
            Box::new(move |fx| {
                calls.fetch_add(1, Ordering::Relaxed);
                fx.fr.w[0] = value;
                Ok(Loc::frame(0))
            }),
            1,
        ),
        Ty::Enc(0),
    )
}

#[test]
fn memo_hit_evaluates_all_inputs_without_decoding() {
    let (ip, typing) = setup();
    let calls = Arc::new(AtomicUsize::new(0));
    let node = node(&ip, vec![encoded(1, &calls), encoded(2, &calls), encoded(3, &calls)]);
    let program = Program::new(&typing);
    let scope = memo::MemoScope::always("inputs");
    let run = Run::words(&ip, &typing, &program, 1).with_memo(scope.exec_id());
    let mut fx = Fx::new(&run, FrameSize { words: 1, ..FrameSize::default() }, &[], false);
    fx.s0 = &[0];
    let want = Value::tuple(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    assert_eq!(node.eval(&mut fx).unwrap(), want);
    assert_eq!(DECODES.with(Cell::get), 3);
    DECODES.with(|n| n.set(0));
    assert_eq!(node.eval(&mut fx).unwrap(), want);
    assert_eq!(calls.load(Ordering::Relaxed), 6);
    assert_eq!(memo::thread_counts(), (2, 1));
    assert_eq!(DECODES.with(Cell::get), 0, "memo hits must not decode encoded inputs");
}

#[test]
fn unkeyed_inputs_keep_encoded_snapshots_on_both_sides() {
    let (ip, typing) = setup();
    let calls = Arc::new(AtomicUsize::new(0));
    let lazy = Value::LazySet(Arc::new(tlx_value::LazySet::Nat));
    let value = lazy.clone();
    let node = node(&ip, vec![encoded(1, &calls), (Code::V(Box::new(move |_| Ok(value.clone()))), Ty::Val), encoded(3, &calls)]);
    let program = Program::new(&typing);
    let scope = memo::MemoScope::always("unkeyed inputs");
    let run = Run::words(&ip, &typing, &program, 1).with_memo(scope.exec_id());
    let mut fx = Fx::new(&run, FrameSize { words: 1, ..FrameSize::default() }, &[], false);
    fx.s0 = &[0];
    for _ in 0..2 {
        let Value::Tuple(values) = node.eval(&mut fx).unwrap() else { panic!("tuple") };
        assert_eq!(values[0], Value::Int(1));
        assert!(matches!(values[1], Value::LazySet(_)));
        assert_eq!(values[2], Value::Int(3));
    }
    assert_eq!(DECODES.with(Cell::get), 4);
    assert_eq!(memo::thread_counts(), (0, 0));
}

#[test]
fn input_errors_still_precede_a_memo_hit() {
    let (ip, typing) = setup();
    let fail = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let failing = |message: &'static str| {
        let fail = fail.clone();
        (Code::I(Box::new(move |_| if fail.load(Ordering::Relaxed) { Err(EvalError::new(message)) } else { Ok(2) })), Ty::Int)
    };
    let node = node(&ip, vec![encoded(1, &calls), failing("first input error"), failing("second input error")]);
    let program = Program::new(&typing);
    let scope = memo::MemoScope::always("input errors");
    let run = Run::words(&ip, &typing, &program, 1).with_memo(scope.exec_id());
    let mut fx = Fx::new(&run, FrameSize { words: 1, ..FrameSize::default() }, &[], false);
    fx.s0 = &[0];
    node.eval(&mut fx).unwrap();
    fail.store(true, Ordering::Relaxed);
    assert_eq!(node.eval(&mut fx).unwrap_err().message, "first input error");
    assert_eq!(memo::thread_counts(), (1, 0), "an input error must abort before lookup");
}

// Count only this test thread's allocation calls inside a measured section.
// Other concurrently running tests do not affect the count.
struct CountingAllocator;

thread_local! {
    static ALLOCS: Cell<Option<usize>> = const { Cell::new(None) };
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocated() {
    let _ = ALLOCS.try_with(|n| {
        if let Some(count) = n.get() {
            n.set(Some(count + 1));
        }
    });
}

// SAFETY: all operations delegate to System with the original arguments.
unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        allocated();
        // SAFETY: the caller supplied a valid allocation layout.
        unsafe { std::alloc::System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: the caller supplied a live allocation with this layout.
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        allocated();
        // SAFETY: the caller supplied a valid allocation layout.
        unsafe { std::alloc::System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        allocated();
        // SAFETY: the caller supplied a live allocation and a valid new size.
        unsafe { std::alloc::System.realloc(ptr, layout, size) }
    }
}

fn allocations(f: impl FnOnce()) -> usize {
    ALLOCS.with(|n| n.set(Some(0)));
    f();
    ALLOCS.with(|n| n.replace(None).unwrap())
}

#[test]
fn composite_input_hits_avoid_decode_allocations() {
    let (ip, mut typing) = setup();
    let shape = Shape::Tuple(vec![Shape::Tuple(vec![Shape::Int { lo: 0, hi: 10 }; 4].into()); 64].into());
    let sid = typing.id(&shape);
    let n = typing.words(sid) as usize;
    let words = vec![0; n];
    let input = || (Code::E(Box::new(|_| Ok(Loc::cnst(0))), n), Ty::Enc(sid));
    let node = node(&ip, vec![input(), input(), (Code::B(Box::new(|_| Ok(true))), Ty::Bool)]);
    let program = Program::new(&typing);
    let scope = memo::MemoScope::always("composite inputs");
    let run = Run::words(&ip, &typing, &program, 1).with_memo(scope.exec_id());
    let mut fx = Fx::new(&run, FrameSize::default(), &words, false);
    fx.s0 = &[0];
    let want = node.eval(&mut fx).unwrap();
    let plain = allocations(|| assert_eq!(node.eval_plain(&mut fx).unwrap(), want));
    let hit = allocations(|| assert_eq!(node.eval(&mut fx).unwrap(), want));
    eprintln!("composite inputs: {plain} plain allocations, {hit} memo-hit allocations");
    assert!(hit < plain / 4, "a hit must avoid the composite inputs' allocations: {hit} vs {plain}");
}
