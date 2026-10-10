//! TLC's `-dump`: write every distinct reachable state to a file, as text
//! (`State n:` blocks) or as a GraphViz graph (`-dump dot[,actionlabels]
//! [,colorize]`). A `DumpObserver` watches one search
//! (`explore_observed`) and records the states (and transitions) as the
//! search finds them.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use tlx_base::hash::IdMap;

use parking_lot::Mutex;

use crate::explore::SearchObserver;
use crate::model::Model;

#[derive(Clone, Copy, Debug, Default)]
pub struct DumpOptions {
    /// GraphViz output instead of text.
    pub dot: bool,
    /// Label transitions with their action.
    pub action_labels: bool,
    /// Color transitions by action.
    pub colorize: bool,
    /// What reduced the search (`symmetry`), noted at the top of the file:
    /// the dump is of the reduced graph.
    pub reduced: Option<&'static str>,
}

struct Out {
    w: BufWriter<File>,
    /// BFS level of each state written (also the set of written states).
    level: IdMap<u32>,
    /// States per level, in discovery order (dot ranks).
    ranks: Vec<Vec<u64>>,
    err: Option<std::io::Error>,
}

impl Out {
    fn write(&mut self, s: &str) {
        if self.err.is_none() {
            if let Err(e) = self.w.write_all(s.as_bytes()) {
                self.err = Some(e);
            }
        }
    }
}

pub struct DumpObserver<'a, M: Model> {
    model: &'a M,
    opts: DumpOptions,
    out: Mutex<Out>,
}

/// A string as a GraphViz label.
fn dot_label(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

impl<'a, M: Model> DumpObserver<'a, M> {
    /// The dump of a search of `model` to `path`.
    pub fn create(model: &'a M, path: &Path, opts: DumpOptions) -> std::io::Result<DumpObserver<'a, M>> {
        let mut out = Out { w: BufWriter::new(File::create(path)?), level: IdMap::default(), ranks: Vec::new(), err: None };
        if let Some(what) = opts.reduced {
            let note = format!("reduced by {what}; use -tlc for the full graph");
            out.write(&if opts.dot { format!("/* {note} */\n") } else { format!("\\* {note}\n") });
        }
        if opts.dot {
            out.write("strict digraph DiskGraph {\nnode [shape=box,style=rounded]\n");
            if opts.colorize {
                out.write("edge [colorscheme=\"paired12\"]\n");
            }
            out.write("nodesep=0.35;\nsubgraph cluster_graph {\ncolor=\"white\";\n");
        }
        Ok(DumpObserver { model, opts, out: Mutex::new(out) })
    }

    /// Complete the file (dot: the ranks of the levels).
    pub fn finish(self) -> std::io::Result<()> {
        let mut out = self.out.into_inner();
        if self.opts.dot {
            for rank in std::mem::take(&mut out.ranks) {
                let ids: String = rank.iter().rev().map(|fp| format!("{};", *fp as i64)).collect();
                out.write(&format!("{{rank = same; {ids}}}\n"));
            }
            out.write("}\n");
            if self.opts.colorize {
                // A legend of the action colors (by name, as TLC).
                out.write("subgraph cluster_legend {graph[style=bold];label = \"Next State Actions\" style=\"solid\"\n");
                out.write("node [ labeljust=\"l\",colorscheme=\"paired12\",style=filled,shape=record ]\n");
                let mut names: Vec<(&String, usize)> = Vec::new();
                for (i, n) in self.model.action_names().iter().enumerate() {
                    if !names.iter().any(|(m, _)| *m == n) {
                        names.push((n, i));
                    }
                }
                names.sort();
                for (n, i) in names {
                    out.write(&format!("{n} [label=\"{}\",fillcolor={}]\n", dot_label(n), i + 2));
                }
                out.write("}");
            }
            out.write("}");
        }
        if let Some(e) = out.err.take() {
            return Err(e);
        }
        out.w.flush()
    }

    /// Record state `s` (fingerprint `fp`, at depth `depth`) the search
    /// reports as new (or outside the model), generated from the state of
    /// fingerprint `parent`: at the level after its parent's (the parent's
    /// as recorded: a state is recorded once, at the level it was first
    /// found at); without a parent (an initial state, or a representative
    /// chosen at the end of a level), at the level of its depth.
    fn record(&self, s: &M::State, fp: u64, depth: u32, parent: Option<u64>) {
        let init = depth == 1;
        let mut out = self.out.lock();
        let level = match parent {
            Some(p) => out.level.get(&p).map_or(0, |l| l + 1),
            None => depth - 1,
        };
        if out.level.contains_key(&fp) {
            return;
        }
        out.level.insert(fp, level);
        if out.ranks.len() <= level as usize {
            out.ranks.resize(level as usize + 1, Vec::new());
        }
        out.ranks[level as usize].push(fp);
        let text = self.model.render(s);
        if self.opts.dot {
            let label = dot_label(&text);
            let line = if init {
                format!("{} [label=\"{label}\",style = filled]\n", fp as i64)
            } else {
                format!("{} [label=\"{label}\",tooltip=\"{label}\"];\n", fp as i64)
            };
            out.write(&line);
        } else {
            let n = out.level.len();
            out.write(&format!("State {n}:\n{text}\n\n"));
        }
    }
}

impl<M: Model> SearchObserver<M::State> for DumpObserver<'_, M> {
    fn on_new_state(&self, s: &M::State, fp: u64, depth: u32, parent: Option<u64>) {
        self.record(s, fp, depth, parent);
    }

    fn on_edge(&self, from: u64, to: u64, a: u32) {
        if !self.opts.dot {
            return;
        }
        let names = self.model.action_names();
        let (label, color) = match (self.opts.action_labels, self.opts.colorize) {
            (true, true) => (names[a as usize].clone(), (a + 2).to_string()),
            (true, false) => (names[a as usize].clone(), "black".to_string()),
            (false, true) => (String::new(), (a + 2).to_string()),
            (false, false) => (String::new(), "black".to_string()),
        };
        let line = format!("{} -> {} [label=\"{}\",color=\"{color}\",fontcolor=\"{color}\"];\n", from as i64, to as i64, dot_label(&label));
        self.out.lock().write(&line);
    }
}
