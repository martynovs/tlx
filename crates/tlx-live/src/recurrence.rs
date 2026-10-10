//! The negation of a property as a disjunction of checks, as TLC splits it
//! (its `OrderOfSolution`s with their `[]<>` and `<>[]` conditions): each
//! disjunct is a conjunction of recurrence conditions (`[]<>x`: `x` at some
//! position of the cycle), persistence conditions (`<>[]x`: `x` at every
//! position of the cycle) and a rest, which needs a tableau. The conditions
//! need no tableau states: a conjunction of `k` of them in a tableau has up
//! to 2^k states (e.g. the `<>[][A]_v` assumptions of a specification, one
//! per process). Bodies `x` are propositional and may mix state and action
//! atoms, evaluated at the same position (a state and the step leaving it).

use crate::{Bits, Ltl};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Condition<'a> {
    body: &'a Ltl,
    neg: bool,
}

impl Condition<'_> {
    pub fn holds(&self, state: &Bits, step: &Bits) -> bool {
        fn eval(f: &Ltl, state: &Bits, step: &Bits) -> bool {
            match f {
                Ltl::True => true,
                Ltl::False => false,
                Ltl::State(i) => state.get(*i),
                Ltl::Action(i) => step.get(*i),
                Ltl::Not(x) => !eval(x, state, step),
                Ltl::And(xs) => xs.iter().all(|x| eval(x, state, step)),
                Ltl::Or(xs) => xs.iter().any(|x| eval(x, state, step)),
                Ltl::Always(_) | Ltl::Eventually(_) => unreachable!("propositional condition"),
            }
        }
        eval(self.body, state, step) != self.neg
    }
}

/// One disjunct of the negation of a property (see the module doc).
#[derive(Debug, Default)]
pub(crate) struct Disjunct<'a> {
    pub recur: Vec<Condition<'a>>,
    pub persist: Vec<Condition<'a>>,
    /// The other conjuncts, if any: their conjunction (not negated).
    pub rest: Option<Ltl>,
}

/// The most disjuncts the negation of a property is split into; a negation
/// with more (distributing conjunctions over disjunctions) is one disjunct
/// of its top-level conjuncts.
const MAX_DISJUNCTS: usize = 64;

/// The disjuncts of the negation of `prop` (borrowing its bodies; the rest
/// is cloned). The formula is not put in a normal form beyond pushing the
/// negation through the connectives, with at most `MAX_DISJUNCTS`
/// disjuncts.
pub(crate) fn split_negation(prop: &Ltl) -> Vec<Disjunct<'_>> {
    let conjunctions = dnf(prop, true).filter(|d| d.len() <= MAX_DISJUNCTS).unwrap_or_else(|| {
        let mut c = Vec::new();
        conjuncts(prop, true, &mut c);
        vec![c]
    });
    conjunctions.into_iter().map(|c| disjunct(&c)).collect()
}

/// A conjunct: a formula and whether it is negated.
type Lit<'a> = (&'a Ltl, bool);

/// `f` (negated if `neg`) in disjunctive normal form over its temporal
/// subformulas and atoms, if it has at most `MAX_DISJUNCTS` disjuncts
/// (each a list of conjuncts; no disjunct: FALSE, an empty one: TRUE).
fn dnf(f: &Ltl, neg: bool) -> Option<Vec<Vec<Lit<'_>>>> {
    match f {
        Ltl::Not(x) => dnf(x, !neg),
        Ltl::True | Ltl::False => Some(if matches!(f, Ltl::True) != neg { vec![Vec::new()] } else { Vec::new() }),
        Ltl::And(xs) | Ltl::Or(xs) if matches!(f, Ltl::And(_)) != neg => {
            // A conjunction: every combination of its parts' disjuncts.
            let mut out = vec![Vec::new()];
            for x in xs {
                let d = dnf(x, neg)?;
                if out.len().saturating_mul(d.len()) > MAX_DISJUNCTS {
                    return None;
                }
                out = out.iter().flat_map(|a| d.iter().map(move |b| a.iter().chain(b).copied().collect())).collect();
            }
            Some(out)
        }
        Ltl::And(xs) | Ltl::Or(xs) => {
            let mut out = Vec::new();
            for x in xs {
                out.extend(dnf(x, neg)?);
                if out.len() > MAX_DISJUNCTS {
                    return None;
                }
            }
            Some(out)
        }
        _ => Some(vec![vec![(f, neg)]]),
    }
}

/// The top-level conjuncts of `f` (negated if `neg`), into `out`.
fn conjuncts<'a>(f: &'a Ltl, neg: bool, out: &mut Vec<Lit<'a>>) {
    match f {
        Ltl::Not(x) => conjuncts(x, !neg, out),
        Ltl::And(xs) if !neg => xs.iter().for_each(|x| conjuncts(x, neg, out)),
        Ltl::Or(xs) if neg => xs.iter().for_each(|x| conjuncts(x, neg, out)),
        _ => out.push((f, neg)),
    }
}

/// The conditions and the rest of the conjunction `c`.
fn disjunct<'a>(c: &[Lit<'a>]) -> Disjunct<'a> {
    let mut d = Disjunct::default();
    let mut rest = Vec::new();
    for &(f, neg) in c {
        match condition(f, neg) {
            Some((true, cond)) => d.recur.push(cond),
            Some((false, cond)) => d.persist.push(cond),
            None => rest.push(if neg { Ltl::Not(Box::new(f.clone())) } else { f.clone() }),
        }
    }
    d.rest = match rest.len() {
        0 => None,
        1 => rest.pop(),
        _ => Some(Ltl::And(rest)),
    };
    d
}

/// `f` (negated if `neg`) as a recurrence (`(true, x)` for `[]<>x`) or
/// persistence condition (`(false, x)` for `<>[]x`), if it is one.
fn condition(f: &Ltl, neg: bool) -> Option<(bool, Condition<'_>)> {
    match (f, neg) {
        (Ltl::Not(x), _) => condition(x, !neg),
        // []y, with y = <>x for a recurrence.
        (Ltl::Always(y), false) | (Ltl::Eventually(y), true) => inner(y, neg, true).map(|c| (true, c)),
        // <>y, with y = []x for a persistence.
        (Ltl::Eventually(y), false) | (Ltl::Always(y), true) => inner(y, neg, false).map(|c| (false, c)),
        _ => None,
    }
}

/// `y` (negated if `neg`) as `<>x` (`eventually`) or `[]x` with `x`
/// propositional: `x` as a condition.
fn inner(y: &Ltl, neg: bool, eventually: bool) -> Option<Condition<'_>> {
    match y {
        Ltl::Not(z) => inner(z, !neg, eventually),
        Ltl::Eventually(x) | Ltl::Always(x) if (matches!(y, Ltl::Eventually(_)) != neg) == eventually && propositional(x) => {
            Some(Condition { body: x, neg })
        }
        _ => None,
    }
}

fn propositional(f: &Ltl) -> bool {
    match f {
        Ltl::Always(_) | Ltl::Eventually(_) => false,
        Ltl::Not(x) => propositional(x),
        Ltl::And(xs) | Ltl::Or(xs) => xs.iter().all(propositional),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(i: u32) -> Ltl {
        Ltl::State(i)
    }
    fn al(f: Ltl) -> Ltl {
        Ltl::Always(Box::new(f))
    }
    fn ev(f: Ltl) -> Ltl {
        Ltl::Eventually(Box::new(f))
    }
    fn not(f: Ltl) -> Ltl {
        Ltl::Not(Box::new(f))
    }

    #[test]
    fn conditions_and_rest() {
        // ¬(<>[]a /\ <>[]b => []<>c) = <>[]a /\ <>[]b /\ <>[]¬c.
        let prop = Ltl::Or(vec![not(Ltl::And(vec![ev(al(st(0))), ev(al(st(1)))])), al(ev(st(2)))]);
        let d = split_negation(&prop);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].recur.len(), d[0].persist.len()), (0, 3));
        assert!(d[0].rest.is_none());
        let (s, e) = (Bits::from_u64(0b011), Bits::new());
        assert!(d[0].persist[0].holds(&s, &e) && d[0].persist[1].holds(&s, &e) && d[0].persist[2].holds(&s, &e));
        // ¬([]<>a \/ (b ~> c)) = <>[]¬a /\ <>(b /\ []¬c): a persistence and
        // a rest; ¬(<>[]a /\ <>b) = []<>¬a \/ []¬b: two disjuncts.
        let leads = al(Ltl::Or(vec![not(st(1)), ev(st(2))]));
        let prop = Ltl::Or(vec![al(ev(st(0))), leads]);
        let d = split_negation(&prop);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].recur.len(), d[0].persist.len()), (0, 1));
        assert!(d[0].rest.is_some());
        let prop = Ltl::And(vec![ev(al(st(0))), ev(st(1))]);
        let d = split_negation(&prop);
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].recur.len(), d[0].persist.len(), d[0].rest.is_none()), (1, 0, true));
        assert_eq!((d[1].recur.len(), d[1].persist.len(), d[1].rest.is_some()), (0, 0, true));
        // ¬TRUE: no disjunct; ¬FALSE: one without conditions or rest.
        assert!(split_negation(&Ltl::True).is_empty());
        let d = split_negation(&Ltl::False);
        assert!(d.len() == 1 && d[0].recur.is_empty() && d[0].persist.is_empty() && d[0].rest.is_none());
    }

    #[test]
    fn too_many_disjuncts_are_not_distributed() {
        // The negation of a disjunction of 7 conjunctions of two is a
        // conjunction of 7 disjunctions of two: 2^7 disjuncts if
        // distributed; one disjunct of 7 conjuncts instead.
        let prop = Ltl::Or((0..7).map(|i| Ltl::And(vec![ev(st(2 * i)), ev(st(2 * i + 1))])).collect());
        let d = split_negation(&prop);
        assert_eq!(d.len(), 1);
        assert!(d[0].rest.is_some());
    }
}
