//! `-dump`: the states and transitions a `DumpObserver` writes as it
//! watches a search.

use tlx_runtime::dump::{DumpObserver, DumpOptions};
use tlx_runtime::{Model, ModelError};

/// x' = x + 1 or x' = x - 1 (two actions), from 0, violating `x < 3`;
/// with `coarse`, as if its fingerprints were of classes (VIEW, SYMMETRY).
struct Line {
    inv: Vec<String>,
    acts: Vec<String>,
    coarse: bool,
}

impl Line {
    fn new(coarse: bool) -> Line {
        Line { inv: vec!["Inv".into()], acts: vec!["Up".into(), "Down".into()], coarse }
    }
}

impl Model for Line {
    type State = u32;
    tlx_runtime::model_without_checks!(but coarse_fingerprint);

    fn coarse_fingerprint(&self) -> bool {
        self.coarse
    }

    fn init_states(&self, out: &mut dyn FnMut(u32)) -> Result<(), ModelError> {
        out(0);
        Ok(())
    }

    fn successors(&self, s: &u32, out: &mut dyn FnMut(u32, u32) -> bool) -> Result<(), ModelError> {
        if out(s + 1, 0) && *s > 0 {
            out(s - 1, 1);
        }
        Ok(())
    }

    fn fingerprint(&self, s: &u32) -> u64 {
        *s as u64 + 1
    }

    fn check_invariants(&self, s: &u32) -> Result<Option<usize>, ModelError> {
        Ok((*s >= 3).then_some(0))
    }

    fn invariant_names(&self) -> &[String] {
        &self.inv
    }

    fn action_names(&self) -> &[String] {
        &self.acts
    }

    fn render(&self, s: &u32) -> String {
        format!("x = {s}")
    }
}

/// The re-execution of the error trace (`rebuild_trace`) records nothing:
/// every transition is in the graph once, as the search generated it.
#[test]
fn dump_dot_has_each_transition_once_after_a_violation() {
    let m = Line::new(false);
    let path = std::env::temp_dir().join(format!("tlx-dump-trace-{}.dot", std::process::id()));
    let dump = DumpObserver::create(&m, &path, DumpOptions { dot: true, action_labels: true, ..DumpOptions::default() }).unwrap();
    let r = tlx_runtime::explore_observed(&m, &tlx_runtime::Options::default(), &dump);
    dump.finish().unwrap();
    assert_eq!(r.outcome, tlx_runtime::Outcome::InvariantViolated("Inv".into()));
    assert_eq!(r.trace.len(), 4);
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let edges: Vec<&str> = text.lines().filter(|l| l.contains(" -> ")).collect();
    let expected = [
        "1 -> 2 [label=\"Up\",color=\"black\",fontcolor=\"black\"];",
        "2 -> 3 [label=\"Up\",color=\"black\",fontcolor=\"black\"];",
        "2 -> 1 [label=\"Down\",color=\"black\",fontcolor=\"black\"];",
        "3 -> 4 [label=\"Up\",color=\"black\",fontcolor=\"black\"];",
    ];
    assert_eq!(edges, expected, "{text}");
    assert!(text.ends_with("{rank = same; 1;}\n{rank = same; 2;}\n{rank = same; 3;}\n{rank = same; 4;}\n}\n}"), "{text}");
}

/// The text dump: every state found, in the order found, numbered.
#[test]
fn dump_text_lists_the_states_found() {
    let m = Line::new(false);
    let path = std::env::temp_dir().join(format!("tlx-dump-text-{}.dump", std::process::id()));
    let dump = DumpObserver::create(&m, &path, DumpOptions::default()).unwrap();
    let r = tlx_runtime::explore_observed(&m, &tlx_runtime::Options::default(), &dump);
    dump.finish().unwrap();
    let plain = tlx_runtime::explore(&m, &tlx_runtime::Options::default());
    assert_eq!((r.stats, r.outcome), (plain.stats, plain.outcome));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(text, "State 1:\nx = 0\n\nState 2:\nx = 1\n\nState 3:\nx = 2\n\nState 4:\nx = 3\n\n");
}

/// With several workers and coarse fingerprints the search chooses the
/// representatives of the classes at the end of each level (without their
/// parents): they are ranked by their depth, as with one worker.
#[test]
fn dump_dot_ranks_the_representatives_chosen_after_a_level_by_depth() {
    let m = Line::new(true);
    let path = std::env::temp_dir().join(format!("tlx-dump-ordered-{}.dot", std::process::id()));
    let dump = DumpObserver::create(&m, &path, DumpOptions { dot: true, ..DumpOptions::default() }).unwrap();
    let r = tlx_runtime::explore_observed(&m, &tlx_runtime::Options { workers: 2, ..tlx_runtime::Options::default() }, &dump);
    dump.finish().unwrap();
    assert_eq!(r.outcome, tlx_runtime::Outcome::InvariantViolated("Inv".into()));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(text.contains("2 [label=\"x = 1\",tooltip=\"x = 1\"];\n"), "{text}");
    assert!(text.ends_with("{rank = same; 1;}\n{rank = same; 2;}\n{rank = same; 3;}\n{rank = same; 4;}\n}\n}"), "{text}");
}
