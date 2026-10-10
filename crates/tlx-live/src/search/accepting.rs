//! The fair accepting SCC search over a product graph (see [`Search`]):
//! Tarjan's algorithm, refined by the fairness constraints until an SCC
//! either holds a fair accepting cycle, cannot hold one, or can only hold
//! one if the nodes where some strong fairness constraint is enabled are
//! removed.

use super::walk::Walk;
use super::{Prod, DEAD, NONE};
use crate::Fairness;

pub(super) struct Search<'a, P: Prod> {
    pub(super) fair: &'a [Fairness],
    pub(super) p: &'a P,
    region: Vec<u32>,
    next_region: u32,
    index: Vec<u32>,
    low: Vec<u32>,
    scratch: Scratch,
}

/// What `Search::classify` collects over an SCC (kept between SCCs: most
/// SCCs are single nodes).
#[derive(Default)]
struct Scratch {
    acc: Vec<bool>,
    have_node: Vec<bool>,
    have_edge: Vec<bool>,
    en_any: Vec<bool>,
    en_all: Vec<bool>,
    taken: Vec<bool>,
}

/// `v` cleared to `len` copies of `x`.
#[inline]
fn reset(v: &mut Vec<bool>, len: usize, x: bool) {
    v.clear();
    v.resize(len, x);
}

impl<'a, P: Prod> Search<'a, P> {
    /// A search over the product `p` for `fair`, with room for `n` nodes.
    pub(super) fn new(fair: &'a [Fairness], p: &'a P, n: usize) -> Self {
        Search { fair, p, region: vec![0; n], next_region: 1, index: vec![NONE; n], low: vec![0; n], scratch: Scratch::default() }
    }
    pub(super) fn enabled(&self, v: u32, f: &Fairness) -> bool {
        self.p.node_bits(v).get(f.enabled)
    }

    /// Whether edge `e` stays inside region `r` (and may be on a cycle).
    #[inline]
    pub(super) fn inside(&self, e: usize, r: u32) -> bool {
        self.region[self.p.tgt(e) as usize] == r && self.p.allowed(e)
    }

    /// Splits `nodes` (all in region `r`) into SCCs over edges internal to `r`.
    /// Each SCC gets a fresh region; returns them as a flat list plus bounds.
    fn tarjan(&mut self, nodes: &[u32], r: u32) -> (Vec<u32>, Vec<usize>) {
        const DONE: u32 = u32::MAX;
        for &v in nodes {
            self.index[v as usize] = NONE;
        }
        let mut counter = 0u32;
        let mut stack: Vec<u32> = Vec::new();
        let mut call: Vec<(u32, usize)> = Vec::new();
        let mut flat = Vec::with_capacity(nodes.len());
        let mut bounds = vec![0];
        for &root in nodes {
            if self.index[root as usize] != NONE {
                continue;
            }
            self.index[root as usize] = counter;
            self.low[root as usize] = counter;
            counter += 1;
            stack.push(root);
            call.push((root, self.p.out(root).start));
            while let Some(&(v, mut pos)) = call.last() {
                let end = self.p.out(v).end;
                let mut descended = false;
                while pos < end {
                    let e = pos;
                    pos += 1;
                    if !self.inside(e, r) {
                        continue;
                    }
                    let w = self.p.tgt(e);
                    if self.index[w as usize] == NONE {
                        self.index[w as usize] = counter;
                        self.low[w as usize] = counter;
                        counter += 1;
                        stack.push(w);
                        call.last_mut().expect("frame").1 = pos;
                        call.push((w, self.p.out(w).start));
                        descended = true;
                        break;
                    } else if self.low[w as usize] != DONE {
                        let lw = self.index[w as usize];
                        if lw < self.low[v as usize] {
                            self.low[v as usize] = lw;
                        }
                    }
                }
                if descended {
                    continue;
                }
                call.pop();
                let lv = self.low[v as usize];
                if let Some(&(u, _)) = call.last() {
                    if lv < self.low[u as usize] {
                        self.low[u as usize] = lv;
                    }
                }
                if lv == self.index[v as usize] {
                    let nr = self.next_region;
                    self.next_region += 1;
                    loop {
                        let w = stack.pop().expect("tarjan stack");
                        self.low[w as usize] = DONE;
                        self.region[w as usize] = nr;
                        flat.push(w);
                        if w == v {
                            break;
                        }
                    }
                    bounds.push(flat.len());
                }
            }
        }
        (flat, bounds)
    }

    /// Finds a fair accepting SCC (or sub-SCC); returns its nodes and region.
    /// Nodes that cannot be on an accepting cycle (`Prod::live`) are left
    /// out from the start.
    pub(super) fn find(&mut self) -> Option<(Vec<u32>, u32)> {
        let mut live = Vec::with_capacity(self.p.len());
        for v in 0..self.p.len() as u32 {
            if self.p.live(v) {
                live.push(v);
            } else {
                self.region[v as usize] = DEAD;
            }
        }
        let mut work: Vec<(Vec<u32>, u32)> = vec![(live, 0)];
        while let Some((nodes, r)) = work.pop() {
            let (flat, bounds) = self.tarjan(&nodes, r);
            drop(nodes);
            for c in bounds.windows(2) {
                let scc = &flat[c[0]..c[1]];
                let r = self.region[scc[0] as usize];
                match self.classify(scc, r) {
                    Verdict::Dead => {
                        for &v in scc {
                            self.region[v as usize] = DEAD;
                        }
                    }
                    Verdict::Found => return Some((scc.to_vec(), r)),
                    Verdict::Refine(remove) => {
                        let nr = self.next_region;
                        self.next_region += 1;
                        let mut rest = Vec::new();
                        for &v in scc {
                            if remove.iter().any(|f| self.enabled(v, f)) {
                                self.region[v as usize] = DEAD;
                            } else {
                                self.region[v as usize] = nr;
                                rest.push(v);
                            }
                        }
                        if !rest.is_empty() {
                            work.push((rest, nr));
                        }
                    }
                }
            }
        }
        None
    }

    /// Whether the SCC `scc` (region `r`) has a fair accepting cycle
    /// (`Found`), has none (`Dead`), or may have one without the nodes
    /// where some strong fairness constraints are enabled (`Refine`).
    fn classify(&mut self, scc: &[u32], r: u32) -> Verdict {
        let mut sc = std::mem::take(&mut self.scratch);
        let verdict = self.classify_with(&mut sc, scc, r);
        self.scratch = sc;
        verdict
    }

    fn classify_with(&self, sc: &mut Scratch, scc: &[u32], r: u32) -> Verdict {
        let (need_node, need_edge) = self.p.need();
        reset(&mut sc.acc, self.p.num_acc(), false);
        reset(&mut sc.have_node, need_node, false);
        // The conditions on nodes first: most SCCs fail them.
        for &v in scc {
            for (h, &a) in sc.acc.iter_mut().zip(self.p.acc(v)) {
                *h |= a;
            }
            for (i, h) in sc.have_node.iter_mut().enumerate() {
                *h |= self.p.node_cond(v, i);
            }
        }
        if sc.acc.contains(&false) || sc.have_node.contains(&false) {
            return Verdict::Dead;
        }
        let n = self.fair.len();
        reset(&mut sc.en_any, n, false);
        reset(&mut sc.en_all, n, true);
        reset(&mut sc.taken, n, false);
        reset(&mut sc.have_edge, need_edge, false);
        let mut internal = false;
        for &v in scc {
            for (j, f) in self.fair.iter().enumerate() {
                let e = self.enabled(v, f);
                sc.en_any[j] |= e;
                sc.en_all[j] &= e;
            }
            for e in self.p.out(v) {
                if !self.inside(e, r) {
                    continue;
                }
                internal = true;
                for (i, h) in sc.have_edge.iter_mut().enumerate() {
                    *h |= self.p.edge_cond(v, e, i);
                }
                if n > 0 {
                    let bits = self.p.edge_bits(v, e);
                    for (j, f) in self.fair.iter().enumerate() {
                        sc.taken[j] |= bits.get(f.taken);
                    }
                }
            }
        }
        if !internal || sc.have_edge.contains(&false) {
            return Verdict::Dead;
        }
        let mut remove = Vec::new();
        for (j, f) in self.fair.iter().enumerate() {
            if sc.taken[j] {
                continue;
            }
            if !f.strong && sc.en_all[j] {
                return Verdict::Dead;
            }
            if f.strong && sc.en_any[j] {
                remove.push(*f);
            }
        }
        if remove.is_empty() {
            Verdict::Found
        } else {
            Verdict::Refine(remove)
        }
    }

    /// Builds the cycle of the lasso for a fair accepting SCC `scc` (region
    /// `r`) the way TLC does (`LiveWorker.printTrace`), from its start `v0`
    /// (returned with it), the node of the SCC nearest to the roots (the
    /// prefix is the BFS-tree path to it); from `v0`, a greedy depth-first walk
    /// (`dfsPostFix`) collects the cycle's conditions — at each node, those
    /// the node satisfies; then the first edge satisfying a condition not met
    /// yet is taken, else the walk goes on to an unvisited successor (the
    /// last one; when there is none it backtracks, taking the first unvisited
    /// successor of the node it returns to); the visited marks are cleared
    /// whenever a step meets a condition. Finally a shortest path closes the
    /// cycle back to `v0` (`bfsPostFix`). A condition met on the way is not
    /// looked for again, so the cycle does not go around twice.
    pub(super) fn cycle(&self, scc: &[u32], r: u32) -> (u32, Vec<(u32, usize)>) {
        let v0 = *scc.iter().min().expect("non-empty SCC");
        (v0, Walk::new(self, scc).run(self, r, v0))
    }
}

enum Verdict {
    Dead,
    Found,
    /// Strong fairness constraints to enforce by removing their enabled nodes.
    Refine(Vec<Fairness>),
}
