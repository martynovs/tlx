//! Shared test helpers: graph construction and a direct evaluator of formulas
//! and fairness on lassos (ultimately periodic behaviors).
#![allow(dead_code)]

use tlx_live::{Bits, Fairness, Graph, Lasso, Ltl};

/// Builds a graph; `edges` are `(from, to, action bits)`. With `stutter`, a
/// self-loop without action bits is added to every state.
pub fn graph(init: &[u32], states: &[u64], edges: &[(u32, u32, u64)], stutter: bool) -> Graph {
    let mut g = Graph {
        init: init.to_vec(),
        state_bits: states.iter().map(|&b| Bits::from_u64(b)).collect(),
        edges: vec![Vec::new(); states.len()],
    };
    for &(s, t, b) in edges {
        g.edges[s as usize].push((t, Bits::from_u64(b)));
    }
    if stutter {
        for s in 0..states.len() {
            g.edges[s].push((s as u32, Bits::new()));
        }
    }
    g
}

/// A position of a lasso: state and bits of the step leaving it.
struct Pos<'a> {
    state: &'a Bits,
    step: &'a Bits,
}

/// Truth of `f` at every position; positions `p..` form the cycle.
fn eval(f: &Ltl, pos: &[Pos], p: usize) -> Vec<bool> {
    let n = pos.len();
    match f {
        Ltl::True => vec![true; n],
        Ltl::False => vec![false; n],
        Ltl::State(a) => pos.iter().map(|x| x.state.get(*a)).collect(),
        Ltl::Action(a) => pos.iter().map(|x| x.step.get(*a)).collect(),
        Ltl::Not(g) => eval(g, pos, p).into_iter().map(|b| !b).collect(),
        Ltl::And(gs) | Ltl::Or(gs) => {
            let conj = matches!(f, Ltl::And(_));
            let mut r = vec![conj; n];
            for g in gs {
                for (x, y) in r.iter_mut().zip(eval(g, pos, p)) {
                    *x = if conj { *x && y } else { *x || y };
                }
            }
            r
        }
        Ltl::Always(g) | Ltl::Eventually(g) => {
            let always = matches!(f, Ltl::Always(_));
            let v = eval(g, pos, p);
            let on_cycle = if always { v[p..].iter().all(|&b| b) } else { v[p..].iter().any(|&b| b) };
            let mut r = vec![on_cycle; n];
            for i in (0..p).rev() {
                let later = r[i + 1];
                r[i] = if always { v[i] && later } else { v[i] || later };
            }
            r
        }
    }
}

/// Whether `l` is a behavior of `g` that is fair and violates `prop`.
pub fn is_fair_violation(g: &Graph, prop: &Ltl, fair: &[Fairness], l: &Lasso) -> bool {
    lasso_error(g, prop, fair, l).is_none()
}

/// `None` if `l` is a valid fair violating lasso, otherwise the reason.
pub fn lasso_error(g: &Graph, prop: &Ltl, fair: &[Fairness], l: &Lasso) -> Option<String> {
    if l.cycle.is_empty() {
        return Some("empty cycle".into());
    }
    let all: Vec<(u32, u32)> = l.prefix.iter().chain(&l.cycle).copied().collect();
    if !g.init.contains(&all[0].0) {
        return Some("does not start in an initial state".into());
    }
    let p = l.prefix.len();
    let mut pos = Vec::new();
    for (i, &(s, e)) in all.iter().enumerate() {
        let next = if i + 1 < all.len() { all[i + 1].0 } else { all[p].0 };
        let Some((t, bits)) = g.edges[s as usize].get(e as usize) else {
            return Some(format!("bad edge index at {i}"));
        };
        if *t != next {
            return Some(format!("edge at {i} goes to {t}, not {next}"));
        }
        pos.push(Pos { state: &g.state_bits[s as usize], step: bits });
    }
    if eval(prop, &pos, p)[0] {
        return Some("property holds on the lasso".into());
    }
    if !fair_on(fair, &pos, p) {
        return Some("lasso is not fair".into());
    }
    None
}

fn fair_on(fair: &[Fairness], pos: &[Pos], p: usize) -> bool {
    fair.iter().all(|f| {
        let cyc = &pos[p..];
        let taken = cyc.iter().any(|x| x.step.get(f.taken));
        let en = if f.strong { cyc.iter().any(|x| x.state.get(f.enabled)) } else { cyc.iter().all(|x| x.state.get(f.enabled)) };
        taken || !en
    })
}

/// Searches all lassos with at most `max_len` positions for a fair violation.
pub fn brute_force(g: &Graph, prop: &Ltl, fair: &[Fairness], max_len: usize) -> Option<Lasso> {
    fn go(g: &Graph, prop: &Ltl, fair: &[Fairness], max_len: usize, path: &mut Vec<(u32, u32)>, s: u32) -> Option<Lasso> {
        for (e, (t, _)) in g.edges[s as usize].iter().enumerate() {
            path.push((s, e as u32));
            for p in 0..path.len() {
                if path[p].0 == *t {
                    let l = Lasso { prefix: path[..p].to_vec(), cycle: path[p..].to_vec() };
                    if is_fair_violation(g, prop, fair, &l) {
                        return Some(l);
                    }
                }
            }
            if path.len() < max_len {
                if let Some(l) = go(g, prop, fair, max_len, path, *t) {
                    return Some(l);
                }
            }
            path.pop();
        }
        None
    }
    g.init.iter().find_map(|&s| go(g, prop, fair, max_len, &mut Vec::new(), s))
}

pub fn st(a: u32) -> Ltl {
    Ltl::State(a)
}
pub fn act(a: u32) -> Ltl {
    Ltl::Action(a)
}
pub fn not(f: Ltl) -> Ltl {
    Ltl::Not(Box::new(f))
}
pub fn always(f: Ltl) -> Ltl {
    Ltl::Always(Box::new(f))
}
pub fn eventually(f: Ltl) -> Ltl {
    Ltl::Eventually(Box::new(f))
}
pub fn or(fs: Vec<Ltl>) -> Ltl {
    Ltl::Or(fs)
}
pub fn and(fs: Vec<Ltl>) -> Ltl {
    Ltl::And(fs)
}
/// `p ~> q` = `[](~p \/ <>q)`.
pub fn leads_to(p: Ltl, q: Ltl) -> Ltl {
    always(or(vec![not(p), eventually(q)]))
}
pub fn wf(enabled: u32, taken: u32) -> Fairness {
    Fairness { strong: false, enabled, taken }
}
pub fn sf(enabled: u32, taken: u32) -> Fairness {
    Fairness { strong: true, enabled, taken }
}
