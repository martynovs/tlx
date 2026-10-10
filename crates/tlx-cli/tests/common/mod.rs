//! Helpers shared by the liveness and coverage CLI tests (each test file
//! uses a different subset).
#![allow(dead_code, reason = "every including test file uses only some of the helpers")]

use tlx_testkit::cli::{tests_specs, Run};

/// Check `tests/specs/<name>.tla` with `args`.
pub fn run_spec(name: &str, args: &[&str]) -> Run {
    let mut cmd = tlx_testkit::tlx!();
    cmd.args(["check", &format!("{name}.tla")]).args(args).dir(tests_specs());
    cmd.run()
}

/// The lines of a liveness result: errors, counterexample states, lasso
/// markers and the final counts (testkit's `result_lines` omits `Temporal`,
/// `Back to state` and `Stuttering`).
pub fn liveness_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| {
            l.starts_with("Error:")
                || l.starts_with("Temporal ")
                || l.starts_with("State ")
                || l.starts_with("/\\ ")
                || l.starts_with("Back to state")
                || l.contains("Stuttering")
                || (l.ends_with("states left on queue.") && !l.starts_with("Progress"))
                || l.starts_with("The depth")
        })
        .map(String::from)
        .collect()
}

/// The lines of the coverage statistics (between the header and
/// `End of statistics`).
pub fn coverage_lines(text: &str) -> Vec<String> {
    text.lines()
        .skip_while(|l| !l.starts_with("The coverage statistics at "))
        .skip(1)
        .take_while(|l| !l.starts_with("End of statistics"))
        .map(String::from)
        .collect()
}

/// The liveness specs of `tests/specs` checked by breadth-first search (no
/// flags, not simulated) whose names are one of `prefixes` and digits.
pub fn liveness_specs(prefixes: &[&str]) -> Vec<String> {
    let mut names = Vec::new();
    for e in std::fs::read_dir(tests_specs()).expect("tests/specs") {
        let p = e.expect("entry").path();
        let Some(name) = p.file_stem().and_then(|s| s.to_str()).map(String::from) else { continue };
        let is_live =
            prefixes.iter().any(|pre| name.strip_prefix(pre).is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())));
        if p.extension().is_none_or(|x| x != "tla") || !is_live {
            continue;
        }
        let flags = std::fs::read_to_string(p.with_extension("flags")).unwrap_or_default();
        if flags.lines().any(|l| !l.starts_with('#') && !l.trim().is_empty()) || flags.contains("# env") {
            continue;
        }
        names.push(name);
    }
    names.sort();
    names
}
