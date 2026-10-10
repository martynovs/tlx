use super::formula::F;
use crate::tableau::{AState, Automaton};

// ---------------------------------------------------------------------------
// TLC's particle tableau (Manna & Pnueli), `TBPar` and `TBGraph`.

type Id = u32;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum T {
    Bool(bool),
    Lit { action: bool, atom: u32, pos: bool },
    And(Id, Id),
    Or(Id, Id),
    All(Id),
    Even(Id),
    Next(Id),
}

#[derive(Default)]
struct Arena {
    nodes: tlx_base::hash::Interner<T, T, std::collections::hash_map::RandomState>,
}

/// `(a, b, c)`: `a` holds iff `b` and `c` (alpha) / `b` or `c` (beta) do.
type Triple = (Id, Id, Id);

impl Arena {
    fn intern(&mut self, t: T) -> Id {
        self.nodes.intern(t)
    }

    /// TLC's `makeBinary`.
    fn binary(&mut self, f: &F) -> Id {
        match f {
            F::Bool(b) => self.intern(T::Bool(*b)),
            &F::Lit { action, atom, pos } => self.intern(T::Lit { action, atom, pos }),
            F::And(xs) | F::Or(xs) => {
                let conj = matches!(f, F::And(_));
                if xs.len() == 1 {
                    return self.binary(&xs[0]);
                }
                let mid = xs.len() / 2;
                let l = self.binary(&F::junction(conj, xs[..mid].iter().cloned()));
                let r = self.binary(&F::junction(conj, xs[mid..].iter().cloned()));
                self.intern(if conj { T::And(l, r) } else { T::Or(l, r) })
            }
            F::All(x) => {
                let b = self.binary(x);
                self.intern(T::All(b))
            }
            F::Even(x) => {
                let b = self.binary(x);
                self.intern(T::Even(b))
            }
        }
    }

    fn positive_closure(&mut self, terms: &[Id]) -> Vec<Id> {
        let mut tps = terms.to_vec();
        let mut result = Vec::new();
        while let Some(ln) = tps.pop() {
            match self.nodes[ln as usize].clone() {
                T::Next(b) => {
                    result.push(ln);
                    tps.push(b);
                }
                T::Even(b) | T::All(b) => {
                    result.push(ln);
                    result.push(self.intern(T::Next(ln)));
                    tps.push(b);
                }
                T::And(a, b) | T::Or(a, b) => {
                    tps.push(a);
                    tps.push(b);
                    result.push(ln);
                }
                T::Bool(_) | T::Lit { .. } => result.push(ln),
            }
        }
        result
    }

    fn triples(&mut self, pc: &[Id], alpha: bool) -> Vec<Triple> {
        let mut ts = Vec::new();
        for &ln in pc {
            match self.nodes[ln as usize].clone() {
                T::All(b) if alpha => ts.push((ln, b, self.intern(T::Next(ln)))),
                T::And(a, b) if alpha => ts.push((ln, a, b)),
                T::Even(b) if !alpha => ts.push((ln, b, self.intern(T::Next(ln)))),
                T::Or(a, b) if !alpha => ts.push((ln, a, b)),
                _ => {}
            }
        }
        ts
    }

    fn locally_consistent(&self, par: &[Id]) -> bool {
        let lits: Vec<(bool, u32, bool)> = par
            .iter()
            .filter_map(|&x| match self.nodes[x as usize] {
                T::Lit { action, atom, pos } => Some((action, atom, pos)),
                _ => None,
            })
            .collect();
        !lits.iter().any(|&(a, x, p)| p && lits.contains(&(a, x, false)))
    }

    /// TLC's `TBPar.particleClosure`.
    fn particle_closure(&mut self, terms: Vec<Id>) -> Vec<Vec<Id>> {
        let pc = self.positive_closure(&terms);
        let alphas = self.triples(&pc, true);
        let betas = self.triples(&pc, false);
        self.closure(terms, &alphas, &betas)
    }

    fn closure(&mut self, terms: Vec<Id>, alphas: &[Triple], betas: &[Triple]) -> Vec<Vec<Id>> {
        if !self.locally_consistent(&terms) {
            return Vec::new();
        }
        let mut t1 = terms.clone();
        let mut i = 0;
        while i < t1.len() {
            let ln = t1[i];
            let kappa = match self.nodes[ln as usize] {
                T::All(b) => Some((b, self.intern(T::Next(ln)))),
                T::And(a, b) => Some((a, b)),
                _ => None,
            };
            if let Some((k1, k2)) = kappa {
                match (t1.contains(&k1), t1.contains(&k2)) {
                    (true, true) => {}
                    (true, false) => t1.push(k2),
                    (false, true) => t1.push(k1),
                    (false, false) => {
                        t1.push(k1);
                        t1.push(k2);
                    }
                }
            }
            i += 1;
        }
        loop {
            let mut done = true;
            for &(a, b, c) in alphas {
                if t1.contains(&b) && t1.contains(&c) && !t1.contains(&a) {
                    t1.push(a);
                    done = false;
                }
            }
            if done {
                break;
            }
        }
        if t1.len() > terms.len() && !self.locally_consistent(&t1) {
            return Vec::new();
        }
        self.closure_beta(t1, alphas, betas)
    }

    fn closure_beta(&mut self, terms: Vec<Id>, alphas: &[Triple], betas: &[Triple]) -> Vec<Vec<Id>> {
        for &ln in &terms {
            let kappa = match self.nodes[ln as usize] {
                T::Even(b) => Some((b, self.intern(T::Next(ln)))),
                T::Or(a, b) => Some((a, b)),
                _ => None,
            };
            if let Some((k1, k2)) = kappa {
                if !terms.contains(&k1) && !terms.contains(&k2) {
                    let with = |k| {
                        let mut t = terms.clone();
                        t.push(k);
                        t
                    };
                    let ps1 = self.closure(with(k1), alphas, betas);
                    let ps2 = self.closure(with(k2), alphas, betas);
                    let mut res: Vec<Vec<Id>> = ps1.into_iter().filter(|p| !ps2.iter().any(|q| same_par(p, q))).collect();
                    res.extend(ps2);
                    return res;
                }
            }
        }
        for &(a, b, c) in betas {
            if (terms.contains(&b) || terms.contains(&c)) && !terms.contains(&a) {
                let mut t = terms.clone();
                t.push(a);
                return self.closure(t, alphas, betas);
            }
        }
        vec![terms]
    }

    /// The promises (`<>` subformulas) of `f`, in TLC's order.
    fn promises(&self, f: Id, out: &mut Vec<Id>) {
        match self.nodes[f as usize] {
            T::Even(b) => {
                if !out.contains(&f) {
                    out.push(f);
                }
                self.promises(b, out);
            }
            T::And(a, b) | T::Or(a, b) => {
                self.promises(a, out);
                self.promises(b, out);
            }
            T::All(b) | T::Next(b) => self.promises(b, out),
            T::Bool(_) | T::Lit { .. } => {}
        }
    }
}

fn same_par(p: &[Id], q: &[Id]) -> bool {
    p.iter().all(|x| q.contains(x)) && q.iter().all(|x| p.contains(x))
}

/// An atom index that is never set: the literal of `FALSE`.
const NEVER: u32 = u32::MAX;

/// Only the initial particles: checking initial consistency must not build
/// the entire reachable tableau before the state search has even started.
pub(super) fn initial_checks(tf: &F) -> Vec<Vec<(u32, bool)>> {
    let mut ar = Arena::default();
    let root = ar.binary(tf);
    ar.particle_closure(vec![root])
        .iter()
        .map(|par| {
            par.iter()
                .filter_map(|&x| match ar.nodes[x as usize] {
                    T::Bool(false) => Some((NEVER, true)),
                    T::Lit { action: false, atom, pos } => Some((atom, pos)),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

/// TLC's tableau (`TBGraph`) for the tableau formula `tf`, as an automaton:
/// node literals are the particle's state predicates (and, beyond TLC, its
/// actions, checked on the step leaving the node); acceptance set i holds
/// the nodes fulfilling promise i. Also which nodes are accepting sinks
/// (`TBGraphNode.isAccepting`).
pub(super) fn particle_tableau(tf: &F) -> (Automaton, Vec<bool>) {
    let mut ar = Arena::default();
    let root = ar.binary(tf);
    let mut pars = ar.particle_closure(vec![root]);
    let init_cnt = pars.len();
    let mut nexts: Vec<Vec<u32>> = Vec::new();
    let mut i = 0;
    while i < pars.len() {
        let imps: Vec<Id> = pars[i]
            .iter()
            .filter_map(|&x| match ar.nodes[x as usize] {
                T::Next(b) => Some(b),
                _ => None,
            })
            .collect();
        let mut succ = Vec::new();
        for par in ar.particle_closure(imps) {
            let j = match pars.iter().position(|p| same_par(p, &par)) {
                Some(j) => j,
                None => {
                    pars.push(par);
                    pars.len() - 1
                }
            };
            succ.push(j as u32);
        }
        nexts.push(succ);
        i += 1;
    }
    let mut promises = Vec::new();
    ar.promises(root, &mut promises);
    let sinks = pars.iter().zip(&nexts).enumerate().map(|(i, (par, succ))| par.is_empty() && succ[..] == [i as u32]).collect();
    let states = pars
        .iter()
        .zip(nexts)
        .map(|(par, succ)| {
            let mut st = AState { state_lits: Vec::new(), action_lits: Vec::new(), succ, acc: Vec::new() };
            for &x in par {
                match ar.nodes[x as usize] {
                    T::Bool(false) => st.state_lits.push((NEVER, true)),
                    T::Lit { action: false, atom, pos } => st.state_lits.push((atom, pos)),
                    T::Lit { action: true, atom, pos } => st.action_lits.push((atom, pos)),
                    _ => {}
                }
            }
            st.acc = promises
                .iter()
                .map(|&p| {
                    let T::Even(b) = ar.nodes[p as usize] else { unreachable!() };
                    !par.contains(&p) || par.contains(&b)
                })
                .collect();
            st
        })
        .collect();
    (Automaton { states, init: (0..init_cnt as u32).collect(), num_acc: promises.len() }, sinks)
}
