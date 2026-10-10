//! Memoization of pure interpreter fallbacks in the IR executor
//! (`exec::memo`, docs/mixed-mode.md §8): purity and read sets, keys
//! (state variables read and bound inputs), hits.

use std::sync::Arc;

use tlx_compile::exec::fast;
use tlx_compile::exec::memo::{analyze, thread_counts, Deps, MemoScope};
use tlx_compile::ir::{Expr, ExprKind, PredFn, Ty};
use tlx_compile::types::Typing;
use tlx_eval::env::Env;
use tlx_eval::Interp;
use tlx_layout::{Layout, Shape};
use tlx_value::Value;

const SPEC: &str = r#"---- MODULE ExecMemo ----
EXTENDS Naturals, TLC
VARIABLES x, y
Init == x = 0 /\ y = 0
Next == x' = x + 1 /\ y' = y
Helper == y * 2
Pure == x + 1
Indirect == Helper + 1
Primed == x' + 1
Vol == TLCGet("level")
Unch == UNCHANGED x
Enab == ENABLED Next
Printing == PrintT(x)
Param(a) == a + x
====
"#;

fn setup(name: &str) -> (Arc<Interp>, Typing) {
    let ip = tlx_testkit::spec::interpreter(name, SPEC, "INIT Init\nNEXT Next\n");
    let int = Shape::Int { lo: 0, hi: 3 };
    (ip, Typing::new(Arc::new(Layout::new(vec![int.clone(), int]))))
}

fn deps(ip: &Interp, name: &str) -> Option<Deps> {
    analyze(ip, tlx_testkit::spec::operator_body(ip, name).0, &[], &[], &Env::default())
}

/// An interpreter-fallback node evaluating the body of `name`, its
/// parameters bound to `args`.
fn interp_def(ip: &Interp, name: &str, args: Vec<Expr>) -> Expr {
    let (e, params) = tlx_testkit::spec::operator_body(ip, name);
    let binds = params.iter().copied().zip(args).collect();
    Expr::new(Ty::Val, ExprKind::Interp { expr: e.clone(), binds, deferred: Vec::new(), env: Env::default() })
}

#[test]
fn purity_and_read_sets() {
    let (ip, _) = setup("purity");
    assert_eq!(deps(&ip, "Pure"), Some(Deps { vars: vec![0] }));
    // Through a definition.
    assert_eq!(deps(&ip, "Indirect"), Some(Deps { vars: vec![1] }));
    for impure in ["Primed", "Vol", "Unch", "Enab", "Printing"] {
        assert_eq!(deps(&ip, impure), None, "{impure}");
    }
    // A parameter bound by the IR is an input, not a state read.
    let (e, params) = tlx_testkit::spec::operator_body(&ip, "Param");
    let binds = vec![(params[0], Expr::new(Ty::Int, ExprKind::Const(Value::Int(3))))];
    assert_eq!(analyze(&ip, e, &binds, &[], &Env::default()), Some(Deps { vars: vec![0] }));
}

#[test]
fn hits_depend_on_the_variables_read_and_the_inputs() {
    let (ip, mut typing) = setup("hits");
    let scope = MemoScope::always("test");
    let result_sid = typing.id(&Shape::Int { lo: 0, hi: 7 });
    let expressions: Vec<_> = [
        interp_def(&ip, "Pure", Vec::new()),
        interp_def(&ip, "Param", vec![Expr::new(Ty::Int, ExprKind::Const(Value::Int(3)))]),
        interp_def(&ip, "Param", vec![Expr::new(Ty::Int, ExprKind::Const(Value::Int(0)))]),
        interp_def(&ip, "Vol", Vec::new()),
    ]
    .into_iter()
    .map(|expression| Expr::new(Ty::Enc(result_sid), ExprKind::Convert(expression)))
    .collect();
    let program = fast::Program::new(&typing);
    let compiled: Vec<_> = expressions.iter().map(|expression| program.expr(&typing, &[], &[], expression).unwrap()).collect();
    let eval = |s0: [u64; 2], index: usize| {
        let mut words = Vec::new();

        // SAFETY: valid words from the matching encoder/program, with published Dyn handles.
        unsafe {
            fast::eval_words(&fast::Run::words(&ip, &typing, &program, 1).with_memo(scope.exec_id()), &compiled[index], &s0, &mut words)
        }
        .unwrap();
        assert_eq!(words.len(), 1);
        Value::Int(words[0] as i64)
    };
    assert_eq!(eval([1, 0], 0), Value::Int(2));
    assert_eq!(thread_counts(), (1, 0));
    assert_eq!(eval([1, 0], 0), Value::Int(2));
    assert_eq!(thread_counts(), (2, 1));
    // `y` is not read: still a hit.
    assert_eq!(eval([1, 3], 0), Value::Int(2));
    assert_eq!(thread_counts(), (3, 2));
    // `x` differs: a miss.
    assert_eq!(eval([2, 3], 0), Value::Int(3));
    assert_eq!(thread_counts(), (4, 2));
    // Bound inputs are part of the key.
    assert_eq!(eval([1, 0], 1), Value::Int(4));
    assert_eq!(eval([1, 0], 2), Value::Int(1));
    assert_eq!(eval([1, 0], 1), Value::Int(4));
    assert_eq!(thread_counts(), (7, 3));
    // Impure nodes are not memoized.
    assert_eq!(eval([1, 0], 3), Value::Int(1));
    assert_eq!(thread_counts(), (7, 3));
}

#[test]
fn off_unless_enabled() {
    let (ip, typing) = setup("off");
    // Scopes made with `new` do not memoize.
    let scope = MemoScope::new("test");
    let pure = interp_def(&ip, "Pure", Vec::new());
    let program = fast::Program::new(&typing);
    let predicate = PredFn {
        name: "off".into(),
        temps: Vec::new(),
        body: Expr::new(Ty::Bool, ExprKind::Eq(pure, Expr::new(Ty::Val, ExprKind::Const(Value::Int(2))))),
    };
    let predicate = program.predicate(&typing, &predicate).expect("closure compilation");
    for _ in 0..2 {
        assert!(
            // SAFETY: valid words from the matching encoder/program, with published Dyn handles.
            unsafe { fast::eval_pred_words(&fast::Run::words(&ip, &typing, &program, 1).with_memo(scope.exec_id()), &predicate, &[1, 0]) }
                .unwrap()
        );
    }
    assert_eq!(thread_counts(), (0, 0));
}
