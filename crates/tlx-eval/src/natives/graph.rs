//! The `Graphs` / `UndirectedGraphs` family (`tlc2/module/Graphs.java` and
//! `UndirectedGraphs.java`): the native graph operators, which quantify over
//! the paths between a graph's nodes (a `[node |-> N, edge |-> E]` record).

use tlx_value::{ops, Sym, Value};

use crate::error::EResult;

/// A graph `[node |-> N, edge |-> E]`: its nodes, and the adjacency
/// between them using only edges whose endpoints are nodes (paths are
/// sequences of nodes). `directed`: edges are pairs `<<a, b>>`, else sets
/// `{a, b}` (or `{a}`, a loop). As in TLC's AbstractGraphs.java
/// `adjacency`, an edge of another shape, or with an endpoint that is not a
/// node, is ignored.
struct Graph {
    nodes: Vec<Value>,
    index: tlx_base::hash::FxMap<Value, usize>,
    adj: Vec<Vec<usize>>,
}

impl Graph {
    fn new(g: &Value, directed: bool) -> EResult<Graph> {
        let nodes = ops::enumerate_normalized(&ops::field(g, Sym::new("node"))?)?;
        let edges = ops::enumerate_normalized(&ops::field(g, Sym::new("edge"))?)?;
        let index: tlx_base::hash::FxMap<Value, usize> = nodes.iter().enumerate().map(|(i, n)| (n.clone(), i)).collect();
        let mut adj = vec![Vec::new(); nodes.len()];
        for e in &edges {
            // Endpoints.of: a pair (Graphs.java `toTuple` of length 2) or a
            // set of one or two nodes (UndirectedGraphs.java `toSetEnum`).
            let ends = if directed {
                match crate::eval::force_seq(e.clone())? {
                    Value::Tuple(t) if t.len() == 2 => t.to_vec(),
                    _ => continue,
                }
            } else {
                match e {
                    Value::Set(_) | Value::Interval(..) | Value::LazySet(_) => match ops::enumerate_normalized(e)?.as_slice() {
                        [a] => vec![a.clone(), a.clone()],
                        [a, b] => vec![a.clone(), b.clone()],
                        _ => continue,
                    },
                    _ => continue,
                }
            };
            if let (Some(&i), Some(&j)) = (index.get(&ends[0]), index.get(&ends[1])) {
                adj[i].push(j);
                if !directed && i != j {
                    adj[j].push(i);
                }
            }
        }
        Ok(Graph { nodes, index, adj })
    }

    fn index(&self, v: &Value) -> Option<usize> {
        self.index.get(v).copied()
    }

    /// Every simple path (a non-empty sequence of distinct nodes, each
    /// joined to the next by an edge), from every node (AbstractGraphs.java
    /// `simplePath` / `extendSimplePath`).
    fn simple_paths(&self) -> EResult<Value> {
        fn extend(g: &Graph, path: &mut Vec<usize>, on: &mut [bool], out: &mut Vec<Value>) {
            out.push(Value::tuple(path.iter().map(|&i| g.nodes[i].clone()).collect()));
            let last = *path.last().unwrap();
            for &next in &g.adj[last] {
                if !on[next] {
                    on[next] = true;
                    path.push(next);
                    extend(g, path, on, out);
                    path.pop();
                    on[next] = false;
                }
            }
        }
        let mut out = Vec::new();
        let mut on = vec![false; self.nodes.len()];
        for i in 0..self.nodes.len() {
            on[i] = true;
            extend(self, &mut vec![i], &mut on, &mut out);
            on[i] = false;
        }
        Ok(Value::set_from(out)?)
    }

    /// Nodes reachable from `i` (including `i`).
    fn reach(&self, i: usize) -> Vec<bool> {
        let mut seen = vec![false; self.nodes.len()];
        let mut stack = vec![i];
        seen[i] = true;
        while let Some(x) = stack.pop() {
            for &y in &self.adj[x] {
                if !seen[y] {
                    seen[y] = true;
                    stack.push(y);
                }
            }
        }
        seen
    }
}

/// `SimplePath(G)` (Graphs / UndirectedGraphs): the set of simple paths.
pub fn simple_path(g: &Value, directed: bool) -> EResult<Value> {
    Graph::new(g, directed)?.simple_paths()
}

/// `AreConnectedIn(m, n, G)`: a non-empty path of nodes from m to n.
pub fn are_connected_in(m: &Value, n: &Value, g: &Value, directed: bool) -> EResult<Value> {
    let g = Graph::new(g, directed)?;
    Ok(Value::Bool(match (g.index(m), g.index(n)) {
        (Some(i), Some(j)) => g.reach(i)[j],
        _ => false,
    }))
}

/// `IsStronglyConnected(G)`: Graphs: every pair connected; UndirectedGraphs:
/// `ConnectedComponents(G) = {G.node}` (so false for an empty graph).
pub fn is_strongly_connected(g: &Value, directed: bool) -> EResult<Value> {
    let g = Graph::new(g, directed)?;
    if directed {
        return Ok(Value::Bool((0..g.nodes.len()).all(|i| g.reach(i).iter().all(|r| *r))));
    }
    Ok(Value::Bool(!g.nodes.is_empty() && g.reach(0).iter().all(|r| *r)))
}

/// `ConnectedComponents(G)` (UndirectedGraphs): the maximal sets of
/// mutually connected nodes.
pub fn connected_components(g: &Value) -> EResult<Value> {
    let g = Graph::new(g, false)?;
    let mut done = vec![false; g.nodes.len()];
    let mut comps = Vec::new();
    for i in 0..g.nodes.len() {
        if done[i] {
            continue;
        }
        let r = g.reach(i);
        let comp: Vec<Value> = (0..g.nodes.len()).filter(|&j| r[j]).map(|j| g.nodes[j].clone()).collect();
        for (j, x) in r.iter().enumerate() {
            done[j] |= *x;
        }
        comps.push(Value::set_from(comp)?);
    }
    Ok(Value::set_from(comps)?)
}
