//! Nondeterministic and side-effecting builtins (RandomElement,
//! RandomSubset, ...) must not be folded into constants at compile time:
//! they are evaluated every time the action runs.

use std::sync::Arc;

use tlx_compile::ir::{ExprKind, Stmt};
use tlx_compile::lower::Lowerer;
use tlx_compile::types::Typing;
use tlx_layout::{Layout, Shape};

const SPEC: &str = r#"---- MODULE Volatile ----
EXTENDS Naturals, FiniteSets, TLC, Randomization
VARIABLE x
Pick(S) == RandomElement(S)
Sub == RandomSubset(3, 1..10)
Init == x = 0
A1 == x' = RandomElement(1..1000)
A2 == x' = Pick(1..1000)
A3 == x' = Cardinality(RandomSubset(3, 1..10))
A4 == x' = Cardinality(RandomSetOfSubsets(2, 2, 1..5))
A5 == x' = LET s == RandomSubset(2, 1..10) IN Cardinality(s)
Next == A1 \/ A2 \/ A3 \/ A4 \/ A5
====
"#;

/// The values assigned in `s`.
fn assigned(s: &Stmt, out: &mut Vec<ExprKind>) {
    if let Stmt::Assign { value, .. } = s {
        out.push((*value.kind).clone());
    }
    s.for_each_sub_stmt(&mut |b| assigned(b, out));
}

#[test]
fn random_builtins_are_not_constants() {
    let model = tlx_testkit::spec::load(SPEC, "INIT Init\nNEXT Next\n");
    let mut lw = Lowerer::with_typing(&model.ip, Typing::new(Arc::new(Layout::new(vec![Shape::Int { lo: 0, hi: 1000 }]))));
    let mut n = 0;
    for a in model.actions() {
        let f = lw.lower_action(a).unwrap_or_else(|e| panic!("lower {}: {}", a.name, e.0));
        let mut vals = Vec::new();
        assigned(&f.body, &mut vals);
        for v in vals {
            n += 1;
            assert!(!matches!(v, ExprKind::Const(_)), "action {}: a random value was folded into a constant", a.name);
        }
    }
    assert_eq!(n, 5);
}
