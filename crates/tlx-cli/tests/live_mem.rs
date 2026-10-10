//! Liveness checking and the memory budget (docs/scale.md §2, §8): a
//! search resumed after a layout miss records the behavior graph too (the
//! levels before its resume point are expanded again after the search),
//! with the same verdicts and counterexamples as an uninterrupted
//! one; a graph that does not fit in `-maxmem` stops liveness checking with
//! a clear error.

use std::path::Path;

/// 6,400 states, strings and counters that outgrow a small pre-run's
/// layout; `Live` holds, `Bad` does not.
const SPEC: &str = r#"---- MODULE LiveMem ----
EXTENDS Naturals, Sequences, TLC
VARIABLES x, y, s
vars == <<x, y, s>>
Init == x = 0 /\ y = 0 /\ s = "n0"
Next == \/ x < 79 /\ x' = x + 1 /\ y' = y /\ s' = "n" \o ToString(x' * 1000 + y')
        \/ y < 79 /\ y' = y + 1 /\ x' = x /\ s' = "n" \o ToString(x' * 1000 + y')
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
Live == <>(x = 79)
Bad == <>(x + y = 200)
Small == x < 3 /\ y < 3
SmallLive == <>(x = 2)
Vw == <<x, y>>
NextP == Next /\ (x + y = 2 => PrintT(<<"at", x, y>>))
SpecP == Init /\ [][NextP]_vars /\ WF_vars(NextP)
====
"#;

/// A directory holding the spec with cfg `cfg` (one per test).
fn spec_dir(_test: &str, cfg: &str) -> tlx_testkit::TempDir {
    tlx_testkit::TempDir::with_spec("live_mem", SPEC, cfg)
}

/// `tlx check LiveMem.tla` with `args` and environment `env`: exit code and
/// output (stdout and stderr).
fn run(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> (i32, String) {
    let mut cmd = tlx_testkit::tlx!();
    cmd.args(["check", "LiveMem.tla", "-config", "LiveMem.cfg"]).args(args).dir(dir);
    let out = cmd.envs(env.iter().copied()).run();
    (out.code, out.text())
}

/// The lines of the result: counts, depth, the violation and its behavior
/// (without times and the statistics).
fn result(out: &str) -> Vec<&str> {
    let skip = [
        "Finished",
        "Starting",
        "Running",
        "Progress",
        "compile:",
        "tlx version",
        "Checking temporal",
        "Implied-temporal",
        "Parsing",
        "Semantic",
        "Computing initial",
        "Finished computing",
    ];
    out.lines().take_while(|l| !l.starts_with("====")).filter(|l| !skip.iter().any(|p| l.starts_with(p)) && !l.contains(" at 20")).collect()
}

/// A resumed search records the behavior graph: no exploration after the
/// search, and the results of an uninterrupted search (the same
/// counterexample with one worker).
#[test]
fn resumed_search_records_the_graph() {
    for prop in ["Live", "Bad"] {
        let dir = spec_dir(&format!("resume-{prop}"), &format!("SPECIFICATION Spec\nPROPERTY {prop}\nCHECK_DEADLOCK FALSE\n"));
        let (code, plain) = run(&dir, &["-compile"], &[]);
        assert_eq!(code, if prop == "Live" { 0 } else { 13 }, "{plain}");
        for workers in ["1", "4"] {
            let (code2, out) = run(&dir, &["-compile", "-workers", workers, "-stats"], &[("TLX_PRERUN", "50")]);
            assert_eq!(code2, code, "{out}");
            assert!(out.contains("compile: resuming the search at depth"), "{out}");
            assert!(out.contains("liveness: states expanded again for their edges"), "{out}");
            assert!(out.contains("liveness: check (recorded graph)"), "{out}");
            assert!(!out.contains("liveness: behavior graph  "), "{out}");
            if workers == "1" {
                assert_eq!(result(&out), result(&plain), "{out}");
            }
        }
    }
}

/// A behavior graph that does not fit in the budget: liveness stops with
/// a clear error (a system error, as a full fingerprint set), in both
/// tiers; the default budget checks the property.
#[test]
fn graph_over_the_budget() {
    let dir = spec_dir("budget", "SPECIFICATION Spec\nPROPERTY Live\nCHECK_DEADLOCK FALSE\n");
    for args in [&["-maxmem", "1"][..], &["-maxmem", "1", "-compile", "-workers", "2"]] {
        let (code, out) = run(&dir, args, &[]);
        assert_eq!(code, 255, "{args:?}:\n{out}");
        assert!(out.contains("Error: liveness needs more memory: with its behavior graph (6400 states, "), "{out}");
        assert!(out.contains("over the memory budget of 1 MiB (-maxmem); rerun with -maxmem "), "{out}");
        assert!(!out.contains("Temporal properties were violated") && !out.contains("State 1:"), "{out}");
    }
    let (code, out) = run(&dir, &["-compile"], &[]);
    assert_eq!(code, 0, "{out}");
}

/// A small graph never fails for the memory the process holds anyway
/// (on Linux, with 8 workers and transparent huge pages, the
/// resident memory alone exceeded a 128 MiB budget): only what liveness
/// adds counts, with at least a quarter of the budget. Here the budget
/// (4 MiB) is below the process's resident memory from the start; with a
/// VIEW, liveness takes TLC's path (`check_concrete`), checked per batch.
#[test]
fn small_graph_under_a_budget_the_process_exceeds() {
    for view in ["", "VIEW Vw\n"] {
        let dir = spec_dir(
            &format!("small{}", view.len()),
            &format!("SPECIFICATION Spec\nCONSTRAINT Small\nPROPERTY SmallLive\nCHECK_DEADLOCK FALSE\n{view}"),
        );
        for args in [&["-maxmem", "4"][..], &["-maxmem", "4", "-compile"], &["-maxmem", "4", "-compile", "-workers", "4"]] {
            let (code, out) = run(&dir, args, &[]);
            assert_eq!(code, 0, "{view} {args:?}:\n{out}");
            assert!(out.contains("Model checking completed. No error has been found."), "{out}");
        }
    }
}

/// The states of the levels before a resume point, expanded again after
/// the search for their liveness edges, do not print again:
/// the output is that of an uninterrupted search.
#[test]
fn expanded_again_without_printing() {
    let dir = spec_dir("print", "SPECIFICATION SpecP\nPROPERTY Live\nCHECK_DEADLOCK FALSE\n");
    let lines = |out: &str| out.lines().filter(|l| l.starts_with("<<\"at\"")).count();
    let (code, plain) = run(&dir, &["-compile"], &[]);
    assert_eq!(code, 0, "{plain}");
    // (Each of the 3 states with x + y = 2 prints once per action evaluated.)
    assert!(lines(&plain) > 0, "{plain}");
    for workers in ["1", "4"] {
        let (code, out) = run(&dir, &["-compile", "-workers", workers, "-stats"], &[("TLX_PRERUN", "50")]);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("compile: resuming the search at depth"), "{out}");
        assert!(out.contains("liveness: states expanded again for their edges"), "{out}");
        assert_eq!(lines(&out), lines(&plain), "{workers} workers:\n{out}");
    }
}

/// A tiny behavior graph can have a large tableau. Its temporary branches
/// must hit the allocation budget before the complete product is built.
/// (The negation of `\E i: [](x = i)` is a conjunction of 20 eventualities,
/// 2^20 tableau states; `[]<>` and `<>[]` conjuncts would be cycle
/// conditions without tableau states. The fairness keeps TLC's own tableau,
/// as large, out of `reports_early`: TLC builds it only for negations with
/// a disjunct without cycle conditions.)
#[test]
fn tableau_over_the_budget() {
    let dir = tlx_testkit::TempDir::with_spec(
        "tableau_budget",
        r#"---- MODULE TableauBudget ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = 0
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Live == \E i \in 1..20: [](x = i)
====
"#,
        "SPECIFICATION Spec\nPROPERTY Live\n",
    );
    for extra in [vec![], vec!["-compile"]] {
        let mut cmd = tlx_testkit::tlx!();
        let out = cmd
            .args(["check", "TableauBudget.tla", "-config", "TableauBudget.cfg", "-maxmem", "1"])
            .args(&extra)
            .dir(&dir)
            .timeout(std::time::Duration::from_secs(15))
            .run();
        assert_eq!(out.code, 255, "{}", out.text());
        assert!(out.text().contains("the liveness automaton and product (0 nodes, 0 edges so far)"), "{}", out.text());
        assert!(!out.text().contains("Temporal properties were violated"), "{}", out.text());
    }
}
