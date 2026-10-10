use crate::{Bits, Fairness, Ltl};

// ---------------------------------------------------------------------------
// Formulas (TLC's LiveExprNode) and their decomposition.

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum F {
    Bool(bool),
    Lit { action: bool, atom: u32, pos: bool },
    And(Vec<F>),
    Or(Vec<F>),
    All(Box<F>),
    Even(Box<F>),
}

const STATE_LEVEL: u8 = 1;
const ACTION_LEVEL: u8 = 2;
const TEMPORAL_LEVEL: u8 = 3;

impl F {
    fn level(&self) -> u8 {
        match self {
            F::Bool(_) => 0,
            F::Lit { action, .. } => {
                if *action {
                    ACTION_LEVEL
                } else {
                    STATE_LEVEL
                }
            }
            F::And(xs) | F::Or(xs) => xs.iter().map(F::level).max().unwrap_or(0),
            F::All(_) | F::Even(_) => TEMPORAL_LEVEL,
        }
    }

    /// A conjunction (`conj`) or disjunction, flattening nested ones (TLC's
    /// `addConj`/`addDisj`).
    pub(super) fn junction(conj: bool, xs: impl IntoIterator<Item = F>) -> F {
        let mut out = Vec::new();
        for x in xs {
            match x {
                F::And(ys) if conj => out.extend(ys),
                F::Or(ys) if !conj => out.extend(ys),
                x => out.push(x),
            }
        }
        if conj {
            F::And(out)
        } else {
            F::Or(out)
        }
    }

    /// Negation normal form of `f` (negated if `neg`): TLC's `pushNeg`.
    fn nnf(f: &Ltl, neg: bool) -> F {
        match f {
            Ltl::True => F::Bool(!neg),
            Ltl::False => F::Bool(neg),
            Ltl::State(a) => F::Lit { action: false, atom: *a, pos: !neg },
            Ltl::Action(a) => F::Lit { action: true, atom: *a, pos: !neg },
            Ltl::Not(g) => F::nnf(g, !neg),
            Ltl::And(gs) => F::junction(!neg, gs.iter().map(|g| F::nnf(g, neg))),
            Ltl::Or(gs) => F::junction(neg, gs.iter().map(|g| F::nnf(g, neg))),
            Ltl::Always(g) => {
                let b = Box::new(F::nnf(g, neg));
                if neg {
                    F::Even(b)
                } else {
                    F::All(b)
                }
            }
            Ltl::Eventually(g) => {
                let b = Box::new(F::nnf(g, neg));
                if neg {
                    F::All(b)
                } else {
                    F::Even(b)
                }
            }
        }
    }

    /// TLC's `simplify`.
    fn simplify(self) -> F {
        match self {
            F::And(xs) | F::Or(xs) if xs.is_empty() => F::Bool(true),
            F::And(xs) => {
                let mut out = Vec::new();
                for x in xs {
                    match x.simplify() {
                        F::Bool(false) => return F::Bool(false),
                        F::Bool(true) => {}
                        y => out.push(y),
                    }
                }
                match out.len() {
                    0 => F::Bool(true),
                    1 => out.pop().unwrap(),
                    _ => F::junction(true, out),
                }
            }
            F::Or(xs) => {
                let mut out = Vec::new();
                for x in xs {
                    match x.simplify() {
                        F::Bool(true) => return F::Bool(true),
                        F::Bool(false) => {}
                        y => out.push(y),
                    }
                }
                match out.len() {
                    0 => F::Bool(false),
                    1 => out.pop().unwrap(),
                    _ => F::junction(false, out),
                }
            }
            F::All(x) => match x.simplify() {
                F::All(y) => F::All(y),
                y => F::All(Box::new(y)),
            },
            F::Even(x) => match x.simplify() {
                F::Even(y) => F::Even(y),
                y => F::Even(Box::new(y)),
            },
            f => f,
        }
    }

    /// `A` if this is `[]<>A`.
    fn ae_body(&self) -> Option<&F> {
        match self {
            F::All(x) => match &**x {
                F::Even(y) => Some(y),
                _ => None,
            },
            _ => None,
        }
    }

    /// TLC's `toDNF` (which does not look below temporal operators and
    /// merges `[]<>A \/ []<>B` into `[]<>(A \/ B)`). Err when a
    /// conjunction has more disjuncts than TLC's `int` can count
    /// (`LNConj.toDNF`'s `Math.multiplyExact`).
    fn into_dnf(self) -> Result<F, DnfTooLarge> {
        Ok(match self {
            F::And(xs) => {
                let mut nes = Vec::new();
                let mut total = 1usize;
                for x in xs {
                    match x.into_dnf()? {
                        F::Or(ds) => {
                            total = total.checked_mul(ds.len()).filter(|&t| t <= i32::MAX as usize).ok_or(DnfTooLarge)?;
                            nes.push(F::Or(ds));
                        }
                        F::And(cs) => nes.extend(cs),
                        y => nes.push(y),
                    }
                }
                if total == 1 {
                    return Ok(F::And(nes));
                }
                let mut res: Vec<Vec<F>> = vec![Vec::new(); total];
                let add = |c: &mut Vec<F>, e: &F| match e {
                    F::And(ys) => c.extend(ys.iter().cloned()),
                    e => c.push(e.clone()),
                };
                let (mut num, mut rcount) = (1, total);
                for ln in &nes {
                    if let F::Or(ds) = ln {
                        rcount /= ds.len();
                        let mut idx = 0;
                        for _ in 0..num {
                            for e in ds {
                                for _ in 0..rcount {
                                    add(&mut res[idx], e);
                                    idx += 1;
                                }
                            }
                        }
                        num *= ds.len();
                    } else {
                        for c in res.iter_mut() {
                            add(c, ln);
                        }
                    }
                }
                F::Or(res.into_iter().map(F::And).collect())
            }
            F::Or(xs) => {
                let (mut res, mut ae) = (Vec::new(), Vec::new());
                let mut add = |e: F| match e.ae_body() {
                    Some(b) => ae.push(b.clone()),
                    None => res.push(e),
                };
                for x in xs {
                    match x.into_dnf()? {
                        F::Or(ds) => ds.into_iter().for_each(&mut add),
                        y => add(y),
                    }
                }
                match ae.len() {
                    0 => {}
                    1 => res.push(F::All(Box::new(F::Even(Box::new(ae.pop().unwrap()))))),
                    _ => res.push(F::All(Box::new(F::Even(Box::new(F::junction(false, ae)))))),
                }
                F::junction(false, res)
            }
            f => f,
        })
    }

    /// TLC's `flattenSingleJunctions`.
    fn flatten_single(self) -> F {
        match self {
            F::And(mut xs) | F::Or(mut xs) if xs.len() == 1 => xs.pop().unwrap().flatten_single(),
            F::And(xs) => F::And(xs.into_iter().map(F::flatten_single).collect()),
            F::Or(xs) => F::Or(xs.into_iter().map(F::flatten_single).collect()),
            F::All(x) => F::All(Box::new(x.flatten_single())),
            F::Even(x) => F::Even(Box::new(x.flatten_single())),
            f => f,
        }
    }

    /// Truth on a step from a state with state-atom bits `s` and
    /// action-atom bits `a` (no temporal operators).
    pub(super) fn eval(&self, s: &Bits, a: &Bits) -> bool {
        match self {
            F::Bool(b) => *b,
            &F::Lit { action, atom, pos } => (if action { a.get(atom) } else { s.get(atom) }) == pos,
            F::And(xs) => xs.iter().all(|x| x.eval(s, a)),
            F::Or(xs) => xs.iter().any(|x| x.eval(s, a)),
            F::All(_) | F::Even(_) => unreachable!("temporal formula in a cycle condition"),
        }
    }
}

/// A possible error model: conditions on a cycle.
#[derive(Debug, Default)]
pub(super) struct Pem {
    /// `[]<>A`, A a state predicate: some state of the cycle satisfies A.
    pub(super) ae_state: Vec<F>,
    /// `[]<>A`, A an action: some step of the cycle satisfies A.
    pub(super) ae_action: Vec<F>,
    /// `<>[]A`: every step of the cycle satisfies A.
    pub(super) ea: Vec<F>,
}

impl Pem {
    /// No condition on cycles: a disjunct of the negated property that a
    /// finite behavior (stuttering at its end) can satisfy.
    pub(super) fn is_empty(&self) -> bool {
        self.ae_state.is_empty() && self.ae_action.is_empty() && self.ea.is_empty()
    }
}

/// TLC's `OrderOfSolution`: a tableau formula (None: no tableau) and the
/// error models checked with it.
#[derive(Debug)]
pub(super) struct Solution {
    pub(super) tf: Option<F>,
    pub(super) pems: Vec<Pem>,
}

/// The state atoms of the tableau formulas TLC builds for `prop` under
/// `fairness`: TLC evaluates them on the successors of each state it
/// expands (tableau node consistency, `LiveCheck.addNextState`) and on the
/// initial states; the other atoms only on the state expanded and its steps.
pub fn tableau_state_atoms(prop: &Ltl, fairness: &[Fairness]) -> Vec<u32> {
    fn collect(f: &F, out: &mut Vec<u32>) {
        match f {
            F::Bool(_) => {}
            F::Lit { action, atom, .. } => {
                if !*action && !out.contains(atom) {
                    out.push(*atom);
                }
            }
            F::And(xs) | F::Or(xs) => xs.iter().for_each(|x| collect(x, out)),
            F::All(x) | F::Even(x) => collect(x, out),
        }
    }
    let mut out = Vec::new();
    for sol in solutions(prop, fairness) {
        if let Some(tf) = &sol.tf {
            collect(tf, &mut out);
        }
    }
    out
}

/// State literals checked, in order, for each initial tableau node.
/// TLC's TBGraphNode.isConsistent short-circuits each node at its first
/// false literal; flattening these lists evaluates predicates TLC skips.
pub fn tableau_initial_checks(prop: &Ltl, fairness: &[Fairness]) -> Vec<Vec<(u32, bool)>> {
    let mut checks = Vec::new();
    for sol in solutions(prop, fairness) {
        if let Some(tf) = &sol.tf {
            checks.extend(super::tableau::initial_checks(tf));
        }
    }
    checks
}

/// A temporal formula whose disjunctive normal form is too large for TLC.
#[derive(Debug)]
pub(super) struct DnfTooLarge;

/// The disjunctive normal form of the negation of `prop` under `fairness`
/// (see `solutions`).
fn negated_dnf(prop: &Ltl, fairness: &[Fairness]) -> Result<F, DnfTooLarge> {
    let ev = |f: F| F::Even(Box::new(f));
    let al = |f: F| F::All(Box::new(f));
    let mut conj: Vec<F> = fairness
        .iter()
        .map(|f| {
            let disabled = F::Lit { action: false, atom: f.enabled, pos: false };
            let taken = al(ev(F::Lit { action: true, atom: f.taken, pos: true }));
            F::Or(vec![if f.strong { ev(al(disabled)) } else { al(ev(disabled)) }, taken])
        })
        .collect();
    conj.push(F::nnf(prop, true));
    F::junction(true, conj).simplify().into_dnf()
}

/// TLC's error for a property whose negation under `fairness` has a
/// disjunctive normal form TLC cannot represent (EC.TLC_LIVE_CANNOT_HANDLE_FORMULA
/// from `LNConj.toDNF`), checked before anything else uses the property.
pub fn check_size(prop: &Ltl, fairness: &[Fairness]) -> Result<(), String> {
    negated_dnf(prop, fairness).map(|_| ()).map_err(|_| {
        "TLC cannot handle the temporal formula because it exceeds the maximum supported size in disjunctive normal form.".to_string()
    })
}

/// The decomposition of the negation of `prop` under `fairness`
/// (`Liveness.processLiveness`). As TLC, fairness is part of the formula:
/// `WF_v(A)` is `[]<>~ENABLED <<A>>_v \/ []<><<A>>_v` (merged into one
/// `[]<>` condition on steps, `ENABLED` evaluated on their first state),
/// `SF_v(A)` is `<>[]~ENABLED <<A>>_v \/ []<><<A>>_v`.
pub(super) fn solutions(prop: &Ltl, fairness: &[Fairness]) -> Vec<Solution> {
    let dnf = negated_dnf(prop, fairness).expect("the size of the formula is checked by check_size");
    if dnf == F::Bool(false) {
        return Vec::new();
    }
    let disjuncts = match dnf {
        F::Or(ds) => ds,
        d => vec![d],
    };
    let mut sols: Vec<Solution> = Vec::new();
    for d in disjuncts {
        let mut pem = Pem::default();
        let mut tfs = Vec::new();
        let conjuncts = match d.flatten_single() {
            F::And(cs) => cs,
            c => vec![c],
        };
        for c in conjuncts {
            match &c {
                F::Even(x) if matches!(&**x, F::All(y) if y.level() < TEMPORAL_LEVEL) => {
                    let F::All(y) = &**x else { unreachable!() };
                    pem.ea.push((**y).clone());
                    continue;
                }
                F::All(x) => {
                    if let F::Even(y) = &**x {
                        match y.level() {
                            l if l <= STATE_LEVEL => {
                                pem.ae_state.push((**y).clone());
                                continue;
                            }
                            ACTION_LEVEL => {
                                pem.ae_action.push((**y).clone());
                                continue;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            tfs.push(c);
        }
        let tf = match tfs.len() {
            0 => None,
            1 => tfs.pop(),
            _ => Some(F::junction(true, tfs)),
        };
        match sols.iter_mut().find(|s| s.tf == tf) {
            Some(s) => s.pems.push(pem),
            None => sols.push(Solution { tf, pems: vec![pem] }),
        }
    }
    sols
}
