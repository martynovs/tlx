use super::*;
use tlx_layout::Shape;

#[test]
fn native_operand_classification_preserves_boxed_boundaries() {
    let boxed = Expr::new(Ty::Val, ExprKind::Const(Value::Int(1)));
    let int = Expr::new(Ty::Int, ExprKind::Const(Value::Int(1)));
    let condition = Expr::new(Ty::Bool, ExprKind::Const(Value::Bool(true)));
    let bin = |left, right| Expr::new(Ty::Int, ExprKind::IntBin(IntOp::Add, left, right));
    assert!(bin(boxed.clone(), int.clone()).has_boxed_operand());
    assert!(bin(int.clone(), boxed.clone()).has_boxed_operand());
    assert!(!bin(int.clone(), int.clone()).has_boxed_operand());
    for kind in [
        ExprKind::Let { temp: 0, value: boxed.clone(), body: int.clone() },
        ExprKind::If(condition.clone(), boxed.clone(), boxed.clone()),
        ExprKind::Eq(boxed.clone(), boxed.clone()),
        ExprKind::Convert(boxed.clone()),
        ExprKind::Enabled(Box::new(Stmt::Guard { cond: boxed.clone(), then: Box::new(Stmt::Stop) })),
        ExprKind::Fail("failure".into()),
        ExprKind::Temp(0),
    ] {
        assert!(!Expr::new(Ty::Val, kind).has_boxed_operand());
    }
    for kind in [
        ExprKind::ConstElems { elem: 0, words: Arc::from([0]), count: 1 },
        ExprKind::IntRange(int.clone(), int.clone()),
        ExprKind::Subsets(Expr::new(Ty::Enc(0), ExprKind::ConstWords(Arc::from([1])))),
    ] {
        let set = Expr::new(Ty::Val, kind);
        let quantifier = Expr::new(Ty::Bool, ExprKind::Exists { bind: 0, set, body: condition.clone() });
        assert!(!quantifier.has_boxed_operand());
    }
    let quantifier = Expr::new(Ty::Bool, ExprKind::Exists { bind: 0, set: boxed, body: condition });
    assert!(quantifier.has_boxed_operand());
}

#[test]
fn scalar_union_arm_uses_the_first_matching_representation() {
    let arms = [Shape::Bool, Shape::Int { lo: -2, hi: 3 }, Shape::Int { lo: 4, hi: 5 }];
    assert_eq!(crate::types::scalar_arm(&arms, Ty::Bool), Some((0, 0, 1)));
    assert_eq!(crate::types::scalar_arm(&arms, Ty::Int), Some((1, -2, 3)));
    assert_eq!(crate::types::scalar_arm(&arms, Ty::Val), None);
    assert_eq!(crate::types::scalar_arm(&arms, Ty::Enc(0)), None);
    assert_eq!(crate::types::scalar_arm(&[], Ty::Int), None);
}
