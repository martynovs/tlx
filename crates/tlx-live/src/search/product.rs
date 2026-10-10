//! One disjunct's product: the reachable CSR product of the behavior graph
//! with the automaton of the disjunct's rest, built part by part (see
//! `super::automaton::AutInfo`), and its [`Prod`] implementation.

use std::ops::Range;

use crate::recurrence::Condition;
use crate::tableau::Automaton;
use crate::{Bits, GraphView};

use super::automaton::{AutInfo, GraphBfs, NodeMap, Part, DENSE_LIMIT};
use super::{Prod, NONE};

/// The parent of a product node entered from a waiting state (see
/// `AutInfo`): the step into its graph state from the graph's BFS tree
/// (`GraphBfs::into`).
const FROM_WAITING: u32 = NONE - 1;

/// One disjunct of the negation of a property to check (see
/// `crate::recurrence`): the automaton of its rest and its conditions.
pub(super) struct Check<'a> {
    pub(super) aut: Automaton,
    pub(super) info: AutInfo,
    /// Its recurrence conditions (`Prod::need`: on edges).
    pub(super) recur: &'a [Condition<'a>],
    /// Its persistence conditions: the product keeps only the edges whose
    /// position satisfies them.
    pub(super) persist: &'a [Condition<'a>],
}

/// The reachable product in CSR form, without the nodes of waiting states
/// and with only the edges inside accepting automaton SCCs (see `AutInfo`)
/// that satisfy the persistence conditions; node ids are in BFS order.
pub(super) struct Product<'a, G: GraphView> {
    g: &'a G,
    aut: &'a Automaton,
    info: &'a AutInfo,
    recurrence: &'a [Condition<'a>],
    /// With waiting states.
    bfs: Option<&'a GraphBfs>,
    pub(super) st: Vec<u32>,
    aq: Vec<u32>,
    /// BFS tree: `(parent node, index of the graph edge from its state)`;
    /// `NONE` for roots, `FROM_WAITING` for nodes entered from a waiting
    /// state.
    parent: Vec<(u32, u32)>,
    /// The BFS distance of each node (while it is built).
    dist: Vec<u32>,
    off: Vec<usize>,
    pub(super) tgt: Vec<u32>,
    /// Index into `edges[st[source]]` of the graph edge behind each product edge.
    pub(super) gedge: Vec<u32>,
}

pub(crate) fn lits_hold(lits: &[(u32, bool)], bits: &Bits) -> bool {
    lits.iter().all(|&(a, pos)| bits.get(a) == pos)
}

impl<'a, G: GraphView> Product<'a, G> {
    /// The memory of the product (bytes, estimated from its allocations),
    /// with the node map, the graph's BFS and what the search adds per node.
    fn bytes(&self, map: &NodeMap) -> usize {
        let bfs = self.bfs;
        let nodes = self.st.capacity() * 4
            + self.aq.capacity() * 4
            + self.parent.capacity() * 8
            + self.dist.capacity() * 4
            + self.off.capacity() * 8;
        self.aut.bytes()
            + self.info.bytes()
            + std::mem::size_of_val(self.recurrence)
            + bfs.map_or(0, GraphBfs::bytes)
            + nodes
            + self.tgt.capacity() * 4
            + self.gedge.capacity() * 4
            + map.bytes()
            + self.st.len() * SEARCH_NODE_BYTES
    }

    /// Node `(s, q)` with BFS tree parent `parent` at distance `dist`,
    /// added unless it exists: its id.
    #[inline]
    fn add(&mut self, map: &mut NodeMap, s: u32, q: u32, parent: (u32, u32), dist: u32) -> u32 {
        let (w, fresh) = map.get_or_insert(s, self.info.slot[q as usize], self.st.len() as u32);
        if fresh {
            self.st.push(s);
            self.aq.push(q);
            self.parent.push(parent);
            self.dist.push(dist);
        }
        w
    }

    /// Build the reachable product of part `part` (`bfs`: the graph's BFS,
    /// if the part is entered from waiting states); `within` (if any) is
    /// called as it grows (every `GUARD_EVERY` nodes, and at the end) with
    /// its numbers of nodes and edges and its memory (`bytes`): None if it
    /// returns false.
    pub(super) fn build(
        g: &'a G,
        ck: &'a Check<'a>,
        part: &'a Part,
        bfs: Option<&'a GraphBfs>,
        mut within: Option<&mut Within<'_>>,
    ) -> Option<Product<'a, G>> {
        let (aut, info, recurrence) = (&ck.aut, &ck.info, ck.recur);
        let slots = part.states.len();
        let map_bytes = g.num_states().checked_mul(slots).filter(|&len| len <= DENSE_LIMIT).unwrap_or(0) * 4;
        let bfs_bytes = bfs.map_or(0, GraphBfs::bytes);
        if let Some(f) = within.as_deref_mut() {
            if !f(0, 0, aut.bytes() + info.bytes() + std::mem::size_of_val(recurrence) + map_bytes + bfs_bytes) {
                return None;
            }
        }
        let bfs = if part.entries.is_empty() { None } else { bfs };
        let mut map = NodeMap::new(g.num_states(), slots);
        let mut p = Product {
            g,
            aut,
            info,
            recurrence,
            bfs,
            st: Vec::new(),
            aq: Vec::new(),
            parent: Vec::new(),
            dist: Vec::new(),
            off: vec![0],
            tgt: Vec::new(),
            gedge: Vec::new(),
        };
        let state_ok = |q: u32, s: u32| lits_hold(&aut.states[q as usize].state_lits, g.state_bits(s));
        let mine = |q: u32| part.states.binary_search(&q).is_ok();
        for &s in g.init() {
            for &q in &aut.init {
                if !info.wait[q as usize] && mine(q) && state_ok(q, s) {
                    p.add(&mut map, s, q, (NONE, 0), 0);
                }
            }
        }
        // The next graph state entered from waiting states (in
        // `GraphBfs::order`).
        let mut entered = 0;
        let mut v = 0;
        loop {
            if let Some(bfs) = &bfs {
                // Enter the nodes of the waiting states' steps at the
                // distance of the next node's successors (with no node left,
                // at the next such distance), keeping the ids in BFS order.
                let upto = match p.dist.get(v) {
                    Some(&d) => d + 1,
                    None if entered < bfs.order.len() => bfs.entry_dist(bfs.order[entered]),
                    None => break,
                };
                while let Some(&t) = bfs.order.get(entered) {
                    let d = bfs.entry_dist(t);
                    if d > upto {
                        break;
                    }
                    entered += 1;
                    for &q in &part.entries {
                        if state_ok(q, t) {
                            p.add(&mut map, t, q, (FROM_WAITING, 0), d);
                        }
                    }
                }
                if v == p.st.len() {
                    continue;
                }
            } else if v == p.st.len() {
                break;
            }
            if v % GUARD_EVERY == GUARD_EVERY - 1 {
                if let Some(f) = within.as_deref_mut() {
                    if !f(p.st.len(), p.tgt.len(), p.bytes(&map)) {
                        return None;
                    }
                }
            }
            let (s, q, d) = (p.st[v], p.aq[v], p.dist[v]);
            let a = &aut.states[q as usize];
            for li in 0..g.degree(s) {
                let (t, bits) = g.edge(s, li);
                if !lits_hold(&a.action_lits, bits) {
                    continue;
                }
                for &q2 in &a.succ {
                    if info.wait[q2 as usize] || !state_ok(q2, t) {
                        continue;
                    }
                    let w = p.add(&mut map, t, q2, (v as u32, li as u32), d + 1);
                    if info.keeps(q, q2) && ck.persist.iter().all(|c| c.holds(g.state_bits(s), bits)) {
                        p.tgt.push(w);
                        p.gedge.push(li as u32);
                    }
                }
            }
            p.off.push(p.tgt.len());
            v += 1;
        }
        if let Some(f) = within {
            if !f(p.st.len(), p.tgt.len(), p.bytes(&map)) {
                return None;
            }
        }
        p.dist = Vec::new();
        Some(p)
    }

    /// The path in the BFS tree from an initial node to node `v`: per
    /// position, the graph state and the index of the graph edge leaving it.
    pub(super) fn prefix(&self, v: u32) -> Vec<(u32, u32)> {
        let mut path = Vec::new();
        let mut v = v;
        loop {
            match self.parent[v as usize] {
                (NONE, _) => break,
                (FROM_WAITING, _) => {
                    let bfs = self.bfs.expect("entered from a waiting state");
                    let mut s = self.st[v as usize];
                    loop {
                        let (u, li) = bfs.into[s as usize];
                        path.push((u, li));
                        s = u;
                        if bfs.dist[s as usize] == 0 {
                            break;
                        }
                    }
                    break;
                }
                (u, li) => {
                    path.push((self.st[u as usize], li));
                    v = u;
                }
            }
        }
        path.reverse();
        path
    }
}

/// A memory check while a product grows (see `Product::build`): given its
/// nodes, edges and bytes, whether to go on.
pub type Within<'a> = dyn FnMut(usize, usize, usize) -> bool + Send + 'a;

/// `Product::build` calls its memory check every this many nodes expanded.
const GUARD_EVERY: usize = 1 << 16;

/// The search's arrays per product node (`Search`: region, index, low).
const SEARCH_NODE_BYTES: usize = 12;

impl<G: GraphView> Prod for Product<'_, G> {
    fn len(&self) -> usize {
        self.st.len()
    }

    #[inline]
    fn out(&self, v: u32) -> Range<usize> {
        self.off[v as usize]..self.off[v as usize + 1]
    }

    #[inline]
    fn tgt(&self, e: usize) -> u32 {
        self.tgt[e]
    }

    #[inline]
    fn live(&self, v: u32) -> bool {
        self.info.live[self.aq[v as usize] as usize]
    }

    #[inline]
    fn acc(&self, v: u32) -> &[bool] {
        &self.aut.states[self.aq[v as usize] as usize].acc
    }

    fn num_acc(&self) -> usize {
        self.aut.num_acc
    }

    fn need(&self) -> (usize, usize) {
        (0, self.recurrence.len())
    }

    #[inline]
    fn edge_cond(&self, v: u32, e: usize, i: usize) -> bool {
        self.recurrence[i].holds(self.node_bits(v), self.edge_bits(v, e))
    }

    #[inline]
    fn node_bits(&self, v: u32) -> &Bits {
        self.g.state_bits(self.st[v as usize])
    }

    #[inline]
    fn edge_bits(&self, v: u32, e: usize) -> &Bits {
        self.g.edge(self.st[v as usize], self.gedge[e] as usize).1
    }
}
