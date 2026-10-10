use std::sync::Arc;

use tlx_layout::{Layout, Shape};
use tlx_value::Value;

use super::*;
use crate::enc::mixed::{RegionCtx, RegionInfo};

fn typing() -> Typing {
    Typing::new(Arc::new(Layout::new(vec![Shape::Int { lo: 0, hi: 2 }])))
}

fn region(e: &Expr) -> Region {
    Region {
        expr: e.clone(),
        temps: Arc::from([]),
        inputs: Vec::new(),
        ctx: RegionCtx::StatePred,
        assigned: Arc::from([]),
        info: RegionInfo::default(),
    }
}

fn konst(b: bool) -> Expr {
    Expr::new(Ty::Bool, ExprKind::Const(Value::Bool(b)))
}

#[test]
fn sites_tell_the_node_and_its_conversion_apart() {
    let e = konst(true);
    assert_eq!(site(&e, false) & 1, 0, "IR nodes are aligned");
    assert_eq!(site(&e, true), site(&e, false) | 1);
    assert_ne!(site(&e, false), site(&konst(true), false));
}

#[test]
fn native_set_any_and_equality() {
    let mut n = NativeSet { actions: vec![false, false], invariants: vec![false], ..Default::default() };
    assert!(!n.any());
    let m = n.clone();
    n.invariants[0] = true;
    assert!(n.any() && n != m);
    assert!(NativeSet { init: true, ..Default::default() }.any());
}

struct Volatile(bool);

impl RegionOracle for Volatile {
    fn reads(&self, _: &Expr) -> Option<Reads> {
        None
    }
    fn stmt_volatile(&self, _: &Stmt) -> bool {
        self.0
    }
    fn expr_volatile(&self, _: &Expr) -> bool {
        self.0
    }
}

#[test]
fn volatile_functions_are_not_mixed() {
    let f = ActionFn { name: "A".into(), temps: Vec::new(), body: Stmt::Stop };
    let p = PredFn { name: "P".into(), temps: Vec::new(), body: konst(true) };
    assert!(Volatile(false).mixes_action(&f) && Volatile(false).mixes_pred(&p));
    assert!(!Volatile(true).mixes_action(&f) && !Volatile(true).mixes_pred(&p));
}

/// The builder keeps regions and call sites in step; the cursor hands out
/// the regions of a site in id order and checks that all were taken.
#[test]
fn regions_are_called_once_at_their_sites() {
    let (a, b, c) = (konst(true), konst(false), konst(true));
    let mut rb = RegionBuilder::default();
    assert_eq!(rb.push(region(&a), site(&a, false)), 0);
    let mark = rb.len();
    rb.push(region(&c), site(&c, true));
    rb.truncate(mark);
    assert_eq!(rb.push(region(&b), site(&b, false)), 1);
    assert_eq!(rb.push(region(&a), site(&a, false)), 2);
    let (table, sites) = rb.finish(&typing());
    assert_eq!((table.len(), rb.len()), (3, 0));
    assert_eq!(sites, [site(&a, false), site(&b, false), site(&a, false)]);

    let plan = NativePlan { native: NativeSet::default(), regions: table, sites };
    let oracle = Volatile(false);
    let mut cur = RegionCursor::new(plan.region_plan(&oracle));
    assert!(cur.has_at(&a) && cur.has(site(&b, false)) && !cur.has(site(&c, true)) && !cur.has_at(&c));
    assert_eq!(cur.take(site(&a, false)), 0);
    assert_eq!(cur.finish().unwrap_err(), "the JIT code calls 1 of the 3 host regions of the C code");
    assert_eq!(cur.take(site(&a, false)), 2);
    assert!(!cur.has_at(&a));
    assert_eq!(cur.take(site(&b, false)), 1);
    assert_eq!(cur.calls(), 3);
    assert!(cur.finish().is_ok());
}
