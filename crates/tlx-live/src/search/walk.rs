//! The walk that turns a fair accepting SCC into a counterexample cycle
//! (see [`Search::cycle`]): the conditions the cycle must meet, the greedy
//! depth-first walk that collects them, and the BFS that closes the cycle
//! back to the node it starts at.

use std::collections::VecDeque;

use super::accepting::Search;
use super::{Prod, NONE};

/// The conditions a fair accepting cycle must meet, and which of them the
/// walk of [`Search::cycle`] has met so far.
pub(super) struct Walk {
    /// Whether each acceptance set and each extra node condition still
    /// needs a node of the walk.
    acc: Vec<bool>,
    node: Vec<bool>,
    /// Whether each extra edge condition still needs a step of the walk.
    edge: Vec<bool>,
    /// Per fairness constraint: whether it still needs a witness (a node
    /// where a weak one is not enabled, or a step taking it).
    fair: Vec<bool>,
    /// The number of conditions still needed.
    left: usize,
}

impl Walk {
    pub(super) fn new<P: Prod>(s: &Search<P>, scc: &[u32]) -> Walk {
        let (need_node, need_edge) = s.p.need();
        // A strong fairness constraint enabled nowhere in the SCC holds
        // without a step taking it.
        let fair: Vec<bool> = s.fair.iter().map(|f| !f.strong || scc.iter().any(|&v| s.enabled(v, f))).collect();
        let left = s.p.num_acc() + need_node + need_edge + fair.iter().filter(|&&n| n).count();
        Walk { acc: vec![true; s.p.num_acc()], node: vec![true; need_node], edge: vec![true; need_edge], fair, left }
    }

    /// Marks the conditions node `v` meets.
    fn at_node<P: Prod>(&mut self, s: &Search<P>, v: u32) {
        for (i, &a) in s.p.acc(v).iter().enumerate() {
            if a && self.acc[i] {
                self.acc[i] = false;
                self.left -= 1;
            }
        }
        for i in 0..self.node.len() {
            if self.node[i] && s.p.node_cond(v, i) {
                self.node[i] = false;
                self.left -= 1;
            }
        }
        for (j, f) in s.fair.iter().enumerate() {
            if self.fair[j] && !f.strong && !s.enabled(v, f) {
                self.fair[j] = false;
                self.left -= 1;
            }
        }
    }

    /// Marks the conditions the edge at CSR position `e` (leaving `v`)
    /// meets; whether it met any.
    fn at_edge<P: Prod>(&mut self, s: &Search<P>, v: u32, e: usize) -> bool {
        let before = self.left;
        for i in 0..self.edge.len() {
            if self.edge[i] && s.p.edge_cond(v, e, i) {
                self.edge[i] = false;
                self.left -= 1;
            }
        }
        if self.fair.contains(&true) {
            let bits = s.p.edge_bits(v, e);
            for (j, f) in s.fair.iter().enumerate() {
                if self.fair[j] && bits.get(f.taken) {
                    self.fair[j] = false;
                    self.left -= 1;
                }
            }
        }
        self.left < before
    }

    /// The cycle from `v0` through region `r` (see [`Search::cycle`]).
    pub(super) fn run<P: Prod>(mut self, s: &Search<P>, r: u32, v0: u32) -> Vec<(u32, usize)> {
        let mut seen = Bfs::new(s.p.len());
        seen.gen = 1;
        seen.stamp[v0 as usize] = seen.gen;
        let mut steps: Vec<(u32, usize)> = Vec::new();
        let mut cur = v0;
        while self.left > 0 {
            self.at_node(s, cur);
            if self.left == 0 {
                break;
            }
            let mut unvisited = None;
            let mut progress = None;
            for e in s.p.out(cur) {
                if !s.inside(e, r) {
                    continue;
                }
                if self.at_edge(s, cur, e) {
                    progress = Some(e);
                    break;
                }
                if seen.stamp[s.p.tgt(e) as usize] != seen.gen {
                    unvisited = Some(e);
                }
            }
            if let Some(e) = progress {
                steps.push((cur, e));
                cur = s.p.tgt(e);
                seen.gen += 1;
                continue;
            }
            let e = loop {
                if let Some(e) = unvisited {
                    break e;
                }
                let (u, _) = steps.pop().expect("conditions met inside the SCC");
                cur = u;
                unvisited = s.p.out(cur).find(|&e| s.inside(e, r) && seen.stamp[s.p.tgt(e) as usize] != seen.gen);
            };
            steps.push((cur, e));
            cur = s.p.tgt(e);
            seen.stamp[cur as usize] = seen.gen;
        }
        let mut bfs = Bfs::new(s.p.len());
        bfs.path(s, r, cur, v0, &mut steps);
        if steps.is_empty() {
            // Nothing to collect: TLC shows the start state stuttering; take
            // its self-loop if it is in the SCC, else any cycle through it.
            let e = s.p.out(v0).find(|&e| s.inside(e, r) && s.p.tgt(e) == v0);
            let e = e.or_else(|| s.p.out(v0).find(|&e| s.inside(e, r))).expect("non-trivial SCC");
            steps.push((v0, e));
            bfs.path(s, r, s.p.tgt(e), v0, &mut steps);
        }
        steps
    }
}

/// BFS scratch space for shortest paths inside a region.
struct Bfs {
    stamp: Vec<u32>,
    gen: u32,
    pred: Vec<(u32, usize)>,
    queue: VecDeque<u32>,
}

impl Bfs {
    fn new(n: usize) -> Self {
        Bfs { stamp: vec![0; n], gen: 0, pred: vec![(NONE, 0); n], queue: VecDeque::new() }
    }

    /// Appends the steps of a shortest path `from -> to` within region `r`.
    fn path<P: Prod>(&mut self, s: &Search<P>, r: u32, from: u32, to: u32, steps: &mut Vec<(u32, usize)>) {
        if from == to {
            return;
        }
        self.gen += 1;
        self.queue.clear();
        self.stamp[from as usize] = self.gen;
        self.queue.push_back(from);
        'bfs: while let Some(v) = self.queue.pop_front() {
            for e in s.p.out(v) {
                let w = s.p.tgt(e);
                if !s.inside(e, r) || self.stamp[w as usize] == self.gen {
                    continue;
                }
                self.stamp[w as usize] = self.gen;
                self.pred[w as usize] = (v, e);
                if w == to {
                    break 'bfs;
                }
                self.queue.push_back(w);
            }
        }
        assert_eq!(self.stamp[to as usize], self.gen, "target reachable inside SCC");
        let start = steps.len();
        let mut w = to;
        while w != from {
            let (v, e) = self.pred[w as usize];
            steps.push((v, e));
            w = v;
        }
        steps[start..].reverse();
    }
}
