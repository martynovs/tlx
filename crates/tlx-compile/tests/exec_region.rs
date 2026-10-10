//! The IR executor's entry points for mixed-mode regions
//! (docs/mixed-mode.md §5): an externally decoded or cached current
//! state, a borrowed next state, the next-state read set, state-predicate
//! contexts and buffer reuse.

use std::cell::Cell;
use std::sync::Arc;

use tlx_compile::exec::{fast, S1View};
use tlx_compile::ir::{Expr, ExprKind, Stmt, Ty};
use tlx_compile::types::Typing;
use tlx_eval::env::Env;
use tlx_eval::Interp;
use tlx_layout::{Layout, Shape};
use tlx_native::build::{build, BuildOptions};
use tlx_native::mixed::{Args, Kind, MixedCx, Out, RegionEnv, RegionError, RegionSig, Regions};
use tlx_native::native::{Mixed, NativeLib};
use tlx_value::Value;

const SPEC: &str = r#"---- MODULE ExecRegion ----
EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y = 0
Next == y' = x + 1
====
"#;

/// The interpreter of the spec and a typing where `x` and `y` are integers
/// in 0..3.
fn setup(name: &str) -> (Arc<Interp>, Typing) {
    let ip = tlx_testkit::spec::interpreter(name, SPEC, "INIT Init\nNEXT Next\n");
    let int = Shape::Int { lo: 0, hi: 3 };
    (ip, Typing::new(Arc::new(Layout::new(vec![int.clone(), int]))))
}

/// An interpreter-fallback node evaluating the body of definition `name`.
fn interp_def(ip: &Interp, name: &str) -> Expr {
    let (e, _) = tlx_testkit::spec::operator_body(ip, name);
    Expr::new(Ty::Val, ExprKind::Interp { expr: e.clone(), binds: Vec::new(), deferred: Vec::new(), env: Env::default() })
}

fn var(i: u32, ty: Ty) -> Expr {
    Expr::new(ty, ExprKind::Var(i))
}

struct Probe<'a> {
    check: &'a dyn Fn(&Args<'_>),
    sig: RegionSig,
}

impl Regions for Probe<'_> {
    fn len(&self) -> u32 {
        1
    }
    fn hash(&self) -> u64 {
        0
    }
    fn sig(&self, _: u32) -> &RegionSig {
        &self.sig
    }
    fn eval(&self, _: u32, args: &Args<'_>, _: &mut RegionEnv<'_>) -> Result<Out, RegionError> {
        (self.check)(args);
        Ok(Out::Bool(true))
    }
}

fn with_args(check: impl Fn(&Args<'_>)) {
    let regions = Probe { check: &check, sig: RegionSig { inputs: vec![Kind::Scalar], result: Kind::Scalar } };
    let source = r#"
#include "tlx_abi.h"
static const char *names[] = {"probe"};
static const tlx_spec_info info = {TLX_ABI_VERSION, 2, 0, 1, 0, names, 0, 0, 1, 0};
const tlx_spec_info *tlx_info(void) { return &info; }
int tlx_init(const tlx_host *h, tlx_ctx *cx) { return TLX_OK; }
int tlx_next(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, uint32_t a) { return TLX_OK; }
int tlx_invariant(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, uint32_t i) {
    tlx_arg input = {.i = 5};
    uint64_t result;
    return h->eval(cx, 0, s, 0, &input, &result) ? TLX_ERR : (int)result;
}
"#;
    let path = build(source, &BuildOptions::default()).expect("probe build");
    // SAFETY: the probe above has static metadata, two-word states and
    // one scalar input/output matching regions; compiler/cache are trusted.
    let library = unsafe { NativeLib::load_with(&path, Some(&regions), None) }.expect("probe load");
    let mut context = MixedCx::new();
    assert_eq!(library.invariant_mixed(Mixed { cx: &mut context, regions: &regions, level: 1 }, &[0, 0], 0), 1);
}

#[test]
fn current_state_is_decoded_once_by_the_given_decoder() {
    let (ip, typing) = setup("decoder");
    let s0 = [2u64, 3];
    let calls = Cell::new(0);
    let decode = |words: &[u64]| {
        calls.set(calls.get() + 1);
        Arc::from(typing.layout.decode_state(words))
    };
    let program = fast::Program::new(&typing);
    let expression = Expr::new(Ty::Val, ExprKind::Tuple(vec![var(0, Ty::Val), var(1, Ty::Val), var(0, Ty::Val)]));
    let expression = program.expr(&typing, &[], &[], &expression).unwrap();
    with_args(|args| {
        let run = fast::Run::words(&ip, &typing, &program, 1).with_decoder(&decode);
        // SAFETY: the matching encoder/program produced these words.
        // Active Dyn handles were published before this call.
        let Out::Val(value) = unsafe { fast::eval_region(&run, &expression, Some(&s0), None, None, args) }.unwrap() else {
            panic!("boxed reads")
        };
        assert_eq!(value, Value::Tuple(vec![Value::Int(2), Value::Int(3), Value::Int(2)].into()));
    });
    assert_eq!(calls.get(), 1);
}

#[test]
fn injected_current_state_values_are_used() {
    let (ip, typing) = setup("inject");
    let values: Arc<[Value]> = vec![Value::Int(1), Value::Int(2)].into();
    let decode = |_: &[u64]| values.clone();
    let program = fast::Program::new(&typing);
    let expression = program.expr(&typing, &[], &[], &interp_def(&ip, "Init")).unwrap();
    with_args(|args| {
        let run = fast::Run::words(&ip, &typing, &program, 1).with_decoder(&decode);
        assert!(matches!(
            // SAFETY: valid words from the matching encoder/program, with published Dyn handles.
            unsafe { fast::eval_region(&run, &expression, Some(&[0, 0]), None, None, args) },
            Ok(Out::Val(Value::Bool(false)))
        ));
        let run = fast::Run::words(&ip, &typing, &program, 1);
        assert!(matches!(
            // SAFETY: valid words from the matching encoder/program, with published Dyn handles.
            unsafe { fast::eval_region(&run, &expression, Some(&[0, 0]), None, None, args) },
            Ok(Out::Val(Value::Bool(true)))
        ));
    });
}

#[test]
fn borrowed_next_state_and_read_set() {
    let (ip, typing) = setup("borrowed");
    let s0 = [1u64, 0];
    let s1 = [0u64, 2];
    let view = S1View { words: &s1, assigned: &[false, true] };
    let program = fast::Program::new(&typing);
    let next = program.expr(&typing, &[], &[], &interp_def(&ip, "Next")).unwrap();
    let boxed = program.expr(&typing, &[], &[], &Expr::new(Ty::Val, ExprKind::NextVar(1))).unwrap();
    let encoded = program.expr(&typing, &[], &[], &Expr::new(typing.var_ty(1), ExprKind::NextVar(1))).unwrap();
    let input = program.expr(&typing, &[Ty::Int], &[0], &Expr::new(Ty::Int, ExprKind::Temp(0))).unwrap();
    with_args(|args| {
        let run = fast::Run::words(&ip, &typing, &program, 1);
        for reads in [None, Some(&[1u32][..])] {
            assert!(
                matches!(
                    // SAFETY: valid words from the matching encoder/program, with published Dyn handles.
                    unsafe { fast::eval_region(&run, &next, Some(&s0), Some(view), reads, args) },
                    Ok(Out::Val(Value::Bool(true)))
                ),
                "reads {reads:?}"
            );
            // SAFETY: the matching encoder/program produced these words.
            // Active Dyn handles were published before this call.
            let Out::Val(value) = unsafe { fast::eval_region(&run, &boxed, Some(&s0), Some(view), reads, args) }.unwrap() else {
                panic!("boxed read")
            };
            assert_eq!(value, Value::Int(2));
            // SAFETY: the matching encoder/program produced these words.
            // Active Dyn handles were published before this call.
            let Out::Words(words) = unsafe { fast::eval_region(&run, &encoded, Some(&s0), Some(view), reads, args) }.unwrap() else {
                panic!("encoded read")
            };
            assert_eq!(&*words, &[2]);
            assert!(matches!(
                // SAFETY: valid words from the matching encoder/program, with published Dyn handles.
                unsafe { fast::eval_region(&run, &input, Some(&s0), Some(view), reads, args) },
                Ok(Out::Int(5))
            ));
        }
        assert!(!matches!(
            // SAFETY: valid words from the matching encoder/program, with published Dyn handles.
            unsafe { fast::eval_region(&run, &next, Some(&s0), Some(view), Some(&[]), args) },
            Ok(Out::Val(Value::Bool(true)))
        ));
    });
}

#[test]
fn assigning_inside_a_region_preserves_borrowed_next_state() {
    let (ip, typing) = setup("assign");
    let s0 = [1u64, 0];
    let s1 = [0u64, 2];
    let view = S1View { words: &s1, assigned: &[false, true] };
    let next_y = Expr::new(typing.var_ty(1), ExprKind::NextVar(1));
    let three = Expr::new(typing.var_ty(0), ExprKind::ConstWords(vec![3u64].into()));
    let two = Expr::new(typing.var_ty(1), ExprKind::ConstWords(vec![2u64].into()));
    let guard = Expr::new(
        Ty::Bool,
        ExprKind::And(vec![
            Expr::new(Ty::Bool, ExprKind::Eq(Expr::new(typing.var_ty(0), ExprKind::NextVar(0)), three.clone())),
            Expr::new(Ty::Bool, ExprKind::Eq(next_y.clone(), two.clone())),
        ]),
    );
    let body = Stmt::Assign {
        var: 0,
        value: three,
        body: Box::new(Stmt::Assign {
            var: 1,
            value: Expr::new(typing.var_ty(1), ExprKind::Temp(0)),
            body: Box::new(Stmt::Guard { cond: guard, then: Box::new(Stmt::Emit) }),
        }),
    };
    let body = Expr::new(
        Ty::Bool,
        ExprKind::And(vec![Expr::new(Ty::Bool, ExprKind::Enabled(Box::new(body))), Expr::new(Ty::Bool, ExprKind::Eq(next_y.clone(), two))]),
    );
    let expression = Expr::new(Ty::Bool, ExprKind::Let { temp: 0, value: next_y, body });
    let program = fast::Program::new(&typing);
    let expression = program.expr(&typing, &[typing.var_ty(1)], &[], &expression).unwrap();
    with_args(|args| {
        let run = fast::Run::words(&ip, &typing, &program, 1);
        assert!(matches!(
            // SAFETY: valid words from the matching encoder/program, with published Dyn handles.
            unsafe { fast::eval_region(&run, &expression, Some(&s0), Some(view), None, args) },
            Ok(Out::Bool(true))
        ));
    });
    assert_eq!(s1, [0, 2]);
}

/// Region equivalence must use operational constant identity, recursively.
/// Equal payloads with different domain witnesses are not interchangeable.
#[test]
fn constant_regions_preserve_domain_witness() {
    use tlx_compile::enc::mixed::{Reads, Region, RegionCtx, RegionInfo, RegionTable};
    use tlx_value::FcnDom;

    let (ip, typing) = setup("constant-witness");
    let pairs: Arc<[(Value, Value)]> = vec![(Value::Int(0), Value::Int(7))].into();
    let enumerated = Value::Fcn(pairs.clone(), FcnDom::Enumerated);
    let interval = Value::Fcn(pairs, FcnDom::Interval);
    let wrap: &[fn(Value) -> Value] = &[
        |v| v,
        |v| Value::tuple(vec![v]),
        |v| Value::Set(vec![v].into()),
        |v| Value::Record(vec![(tlx_value::intern::Sym::new("a"), v)].into()),
        |v| Value::Fcn(vec![(v, Value::Int(1))].into(), FcnDom::Enumerated),
    ];
    for wrap in wrap {
        let region = |v| Region {
            expr: Expr::new(Ty::Val, ExprKind::Const(v)),
            temps: Arc::from([]),
            inputs: vec![],
            ctx: RegionCtx::StatePred,
            assigned: Arc::from([]),
            info: RegionInfo { pure: true, reads: Some(Reads::default()), ..Default::default() },
        };
        let table = RegionTable::new(
            &typing,
            vec![
                region(wrap(enumerated.clone())),
                region(wrap(interval.clone())),
                region(wrap(Value::Fcn(vec![(Value::Int(0), Value::Int(7))].into(), FcnDom::Enumerated))),
            ],
        );
        assert_eq!(table.memo_class(&ip.prog), &[0, 1, 0]);
    }
}

/// Exercise the shared memo consumer behind a real C -> host callback:
/// warming the enumerated constant must not replace the interval constant.
#[test]
fn shared_region_results_retain_lookup_errors() {
    use tlx_compile::enc::mixed::{FastRegions, HostRegions, Reads, Region, RegionCtx, RegionInfo, RegionTable};
    use tlx_compile::exec::memo::{thread_counts, MemoScope};
    use tlx_native::mixed::StateCache;
    use tlx_value::{ops, FcnDom};

    let (ip, typing) = setup("region-witness-error");
    let pairs: Arc<[(Value, Value)]> = vec![(Value::Int(0), Value::Int(7))].into();
    let region = |dom| Region {
        expr: Expr::new(
            Ty::Val,
            ExprKind::If(
                Expr::new(
                    Ty::Bool,
                    ExprKind::Eq(Expr::new(Ty::Int, ExprKind::Temp(0)), Expr::new(Ty::Int, ExprKind::Const(Value::Int(5)))),
                ),
                Expr::new(Ty::Val, ExprKind::Const(Value::Fcn(pairs.clone(), dom))),
                Expr::new(Ty::Val, ExprKind::Const(Value::Fcn(pairs.clone(), dom))),
            ),
        ),
        temps: vec![Ty::Int].into(),
        inputs: vec![0],
        ctx: RegionCtx::StatePred,
        assigned: Arc::from([]),
        info: RegionInfo { pure: true, reads: Some(Reads::default()), ..Default::default() },
    };
    let table = RegionTable::new(&typing, vec![region(FcnDom::Enumerated), region(FcnDom::Interval), region(FcnDom::Enumerated)]);
    let program = fast::Program::new(&typing);
    let slots = FastRegions::slots(&table);
    let scope = MemoScope::always("region witness regression");
    let host = HostRegions {
        ip: &ip,
        typing: &typing,
        table: &table,
        memo: scope.exec_id(),
        fast: Some(FastRegions { prog: &program, progs: &slots }),
    };
    let absent = Value::ModelValue(tlx_value::intern::Sym::new("absent"));
    with_args(|args| {
        let mut cache = StateCache::default();
        let mut env = RegionEnv { s0: Some(&[0, 0]), s1: None, level: 1, cache: &mut cache };
        for rid in [0, 1, 2, 1] {
            let Out::Val(v) = host.eval(rid, args, &mut env).unwrap() else { panic!("function value") };
            assert_eq!(ops::select(&v, &absent).is_err(), rid == 1, "region {rid}");
        }
    });
    assert_eq!(thread_counts(), (4, 2), "same-witness regions share; different witnesses stay separate");
}
