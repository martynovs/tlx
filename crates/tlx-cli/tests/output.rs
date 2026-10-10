//! The messages of `tlx check`, plain and with `-tool`, compared with
//! TLC's (2026.10 output on the same specs).

use tlx_testkit::cli::tests_specs as specs;

fn run(args: &[&str]) -> (String, i32) {
    let out = tlx_testkit::tlx!().arg("check").args(args).dir(specs()).run();
    (out.stdout, out.code)
}

/// The codes and classes of the `-tool` messages, in order.
fn messages(out: &str) -> Vec<(u32, u32)> {
    out.lines()
        .filter_map(|l| l.strip_prefix("@!@!@STARTMSG ")?.strip_suffix(" @!@!@"))
        .map(|m| {
            let (c, k) = m.split_once(':').unwrap();
            (c.parse().unwrap(), k.parse().unwrap())
        })
        .collect()
}

#[test]
fn initial_liveness_error_is_not_replaced_by_diagnostic_replay() {
    // D-27: TLC's diagnostic doInit(true) reuses its fingerprint set,
    // bypasses the failing liveness check, and changes counts or verdict.
    // Keep the actual first error in every execution tier.
    for compile in [false, true] {
        for (spec, error) in [("FzLivePredErrorCounts.tla", "not in the domain"), ("FzLivePredErrorStats.tla", "no element")] {
            let mut args = vec![spec, "-tool"];
            if compile {
                args.push("-compile");
            }
            let (out, code) = run(&args);
            assert_ne!(code, 0, "{out}");
            assert!(out.contains(error), "{out}");
            assert!(!out.contains("Invariant Inv1 is violated"), "{out}");
            assert!(out.contains("1 states generated, 1 distinct states found"), "{out}");
            assert!(out.contains("depth of the complete state graph search is 1"), "{out}");
        }
    }
}

#[test]
fn tool_invariant_violation() {
    let (out, code) = run(&["T4.tla", "-tool"]);
    assert_eq!(code, 12);
    let mut want = vec![(2262, 0), (2187, 0), (2220, 0), (2219, 0), (2185, 0), (2189, 0), (2190, 0), (2110, 1), (2121, 1)];
    want.extend([(2217, 4); 7]);
    want.extend([(2200, 0), (2199, 0), (2194, 0), (2186, 0)]);
    assert_eq!(messages(&out), want, "{out}");
    // Every message is closed.
    assert_eq!(out.matches("@!@!@STARTMSG").count(), out.matches("@!@!@ENDMSG").count());
    assert!(out.contains("@!@!@STARTMSG 2110:1 @!@!@\nInvariant Inv is violated.\n@!@!@ENDMSG 2110 @!@!@\n"), "{out}");
    assert!(out.contains("@!@!@STARTMSG 2217:4 @!@!@\n1: <Initial predicate>\n/\\ x = 0\n/\\ y = 0\n\n@!@!@ENDMSG 2217 @!@!@\n"), "{out}");
    assert!(out.contains("\n28 states generated, 19 distinct states found, 3 states left on queue.\n"), "{out}");
    assert!(out.contains(" 28 states generated ("), "{out}");
}

#[test]
fn tool_liveness_violation() {
    // A lasso closed by "Back to state" and a stuttering one.
    let (out, code) = run(&["S1.tla", "-tool", "-simulate", "num=5", "-depth", "20", "-seed", "1"]);
    assert_eq!(code, 13);
    let m = messages(&out);
    assert!(m.contains(&(2116, 1)) && m.contains(&(2264, 1)) && m.contains(&(2122, 4)), "{out}");
    assert!(out.contains("Back to state: <Next line 6, col 12 to line 6, col 26 of module S1>\n\n@!@!@ENDMSG 2122 @!@!@"), "{out}");
    let (out, _) = run(&["L1.tla", "-tool"]);
    let m = messages(&out);
    for c in [(2200, 0), (2192, 0), (2116, 1), (2264, 1), (2218, 4), (2267, 0)] {
        assert!(m.contains(&c), "{c:?} missing: {out}");
    }
    assert!(out.contains("@!@!@STARTMSG 2218:4 @!@!@\n4: Stuttering\n@!@!@ENDMSG 2218 @!@!@\n"), "{out}");
}

#[test]
fn exit_codes_of_temporal_properties() {
    // A violation found during the search exits as a safety violation, as
    // TLC's (LiveCheck.printErrorTrace sets a safety error state).
    let (out, code) = run(&["TSREarlySafety.tla"]);
    assert_eq!(code, 12, "{out}");
    assert!(out.contains("Error: Temporal property AtMostOnce was violated."), "{out}");
    // A tautology is refused (FAILURE_LIVENESS_EVAL), before the search and
    // while the configuration is processed (a quantifier over {}).
    for spec in ["TSRTautology.tla", "TSRTautologyForall.tla"] {
        let (out, code) = run(&[spec, "-tool"]);
        assert_eq!(code, 77, "{out}");
        assert!(out.contains("@!@!@STARTMSG 2253:1 @!@!@\nTemporal formula is a tautology (its negation is unsatisfiable).\n"), "{out}");
    }
}

#[test]
fn plain_success() {
    let (out, code) = run(&["T2.tla"]);
    assert_eq!(code, 0, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    let at = |prefix: &str| lines.iter().position(|l| l.starts_with(prefix)).unwrap_or_else(|| panic!("no {prefix}: {out}"));
    // TLC's order.
    let order = [
        "Running breadth-first search Model-Checking with ",
        "Starting... (",
        "Computing initial states...",
        "Finished computing initial states: 1 distinct state generated at ",
        "Progress(4) at ",
        "Checking temporal properties for the complete state space with 6 total distinct states at (",
        "Finished checking temporal properties in ",
        "Model checking completed. No error has been found.",
        "  calculated (optimistic):  val = 0.0",
        "6 states generated, 6 distinct states found, 0 states left on queue.",
        "The depth of the complete state graph search is 4.",
        "The average outdegree of the complete state graph is 1 (minimum is 0, the maximum 2 and the 95th percentile is 2).",
        "Finished in ",
    ];
    let pos: Vec<usize> = order.iter().map(|p| at(p)).collect();
    assert!(pos.windows(2).all(|w| w[0] < w[1]), "{out}");
    assert!(!out.contains("@!@!@"));
}

#[test]
fn plain_initial_state_violation() {
    // TLC reports neither the initial states nor statistics.
    let (out, code) = run(&["C1.tla"]);
    assert_eq!(code, 12);
    assert!(out.contains("Error: Invariant Inv is violated by the initial state:\nx = \"f\"\n\nFinished in "), "{out}");
    assert!(!out.contains("states generated"), "{out}");
}

#[test]
fn simulation_with_constraints() {
    // TLC's simulator: all initial states are counted, the CONSTRAINT selects
    // those behaviors start from, and successors outside the constraints
    // count as generated.
    let (out, code) = run(&["S5.tla", "-simulate", "num=3", "-depth", "10"]);
    assert_eq!(code, 0, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    let at = |prefix: &str| lines.iter().position(|l| l.starts_with(prefix)).unwrap_or_else(|| panic!("no {prefix}: {out}"));
    let order = [
        "Running Random Simulation with seed ",
        "Starting... (",
        "Computed 2 initial states...",
        "Finished computing initial states: 1 distinct state generated at ",
        "Progress: 14 states checked, 3 traces generated (trace length: mean=4, var(x)=0, sd=0)",
        "The number of states generated: 14",
        "Finished in ",
    ];
    let pos: Vec<usize> = order.iter().map(|p| at(p)).collect();
    assert!(pos.windows(2).all(|w| w[0] < w[1]), "{out}");
    // A successor outside the CONSTRAINT leaves the state without successors.
    let (out, code) = run(&["S6.tla", "-simulate", "num=1", "-depth", "10"]);
    assert_eq!(code, 11, "{out}");
    assert!(out.contains("Error: Deadlock reached.\nError: The behavior up to this point is:\n"), "{out}");
    assert!(out.contains("x = 2\n\nThe number of states generated: 5\n"), "{out}");
    // No initial state satisfies the CONSTRAINT: no statistics.
    let (out, code) = run(&["S6.tla", "-simulate", "num=1", "-config", "S6b.cfg"]);
    assert_eq!(code, 255, "{out}");
    let want = "Computed 2 initial states...\nError: There is no state satisfying the initial state predicate and the state-constraint(s).\nFinished in ";
    assert!(out.contains(want), "{out}");
}

#[test]
fn variable_print_order() {
    // TLC's order (that of SANY's symbol table), not the declaration order.
    let (out, _) = run(&["O1.tla"]);
    assert!(out.contains("State 1: <Initial predicate>\n/\\ done = FALSE\n/\\ zz = 2\n/\\ a = 3\n/\\ b = 1\n/\\ s = 0\n"), "{out}");
    let (out, _) = run(&["L5.tla"]);
    assert!(out.contains("State 1: <Initial predicate>\n/\\ done = FALSE\n/\\ s = 0\n"), "{out}");
    // It depends on the size of the symbol table: 143 symbols, then 144.
    let (out, _) = run(&["O2.tla"]);
    assert!(out.contains("State 1: <Initial predicate>\n/\\ et = 0\n/\\ aa = 0\n"), "{out}");
    let (out, _) = run(&["O3.tla"]);
    assert!(out.contains("State 1: <Initial predicate>\n/\\ aa = 0\n/\\ et = 0\n"), "{out}");
}

#[test]
fn liveness_counterexample_as_tlc() {
    // A step between two equal states (differing only in the tableau) is not
    // shown.
    let (out, code) = run(&["L10.tla"]);
    assert_eq!(code, 13, "{out}");
    assert!(out.contains("State 3: <Next line 5, col 9 to line 5, col 24 of module L10>\nx = 2\n\nBack to state 1: <Next "), "{out}");
    assert!(!out.contains("Stuttering"), "{out}");
    // No fairness warning when the property needs no cycle condition.
    let (out, _) = run(&["L1.tla"]);
    assert!(out.contains("State 4: Stuttering\n"), "{out}");
    assert!(!out.contains("Warning: The stuttering counterexample"), "{out}");
}

#[test]
fn liveness_counterexample_under_view() {
    // As TLC: the cycle of the VIEW classes 0 -> 1, shown with the states
    // found first (the step back leads to (0, 1)); in simulation too.
    let (out, code) = run(&["VL2.tla"]);
    assert_eq!(code, 13, "{out}");
    let cycle = "\n\nState 2: <Next line 8, col 12 to line 8, col 45 of module VL2>\n/\\ x = 1\n/\\ y = 0\n\nBack to state 1: <Next line 9, col 12 to line 9, col 36 of module VL2>\n";
    assert!(out.contains(&format!("Error: Temporal property Live was violated.\n\nError: The following behavior constitutes a counter-example:\n\nState 1: <Initial predicate>\n/\\ x = 0\n/\\ y = 0{cycle}")), "{out}");
    let (out, code) = run(&["VL6.tla", "-simulate", "num=3", "-depth", "10"]);
    assert_eq!(code, 13, "{out}");
    let cycle = "\n\nState 2: <Next line 10, col 12 to line 10, col 45 of module VL6>\n/\\ x = 1\n/\\ y = 0\n\nBack to state 1: <Next line 11, col 12 to line 11, col 36 of module VL6>\n";
    assert!(out.contains(&format!("State 1: <Init line 9, col 9 to line 9, col 22 of module VL6>\n/\\ x = 0\n/\\ y = 0{cycle}")), "{out}");
}

#[test]
fn liveness_counterexample_under_view_rebuilt() {
    // TLC's algorithm keeps no states: the states of the lasso are
    // rebuilt by re-executing the spec, the last one from a state expanded
    // on demand (h = 0, not the first-found h = 2 of its VIEW class).
    let (out, code) = run(&["RT44View.tla"]);
    assert_eq!(code, 13, "{out}");
    let step = |p: u32| format!("<Step({p}) line 12, col 12 to line 12, col 65 of module RT44View>");
    let want = format!(
        "State 1: <Initial predicate>\n/\\ c = <<0, 0, 0>>\n/\\ h = 0\n\nState 2: {}\n/\\ c = <<0, 1, 0>>\n/\\ h = 2\n\nState 3: {}\n/\\ c = <<1, 1, 0>>\n/\\ h = 3\n\nState 4: {}\n/\\ c = <<2, 1, 0>>\n/\\ h = 4\n\nState 5: {}\n/\\ c = <<0, 1, 0>>\n/\\ h = 0\n\nBack to state 3: {}\n",
        step(2),
        step(1),
        step(1),
        step(1),
        step(1)
    );
    assert!(out.contains(&want), "{out}");
}

const UNEXPECTED: &str ="Error: TLC threw an unexpected exception.\nThis was probably caused by an error in the spec or model.\nSee the User Output or TLC Console for clues to what happened.\nThe exception was a ";

#[test]
fn initial_state_errors() {
    // An error while computing the initial states: no prefix, and the
    // statistics so far (TLC's nested-expression positions are not shown).
    let (out, code) = run(&["EK1.tla"]);
    assert_eq!(code, 75, "{out}");
    let want = "Computing initial states...\nError: The second argument of \\div is 0.\n1 states generated, 1 distinct states found, 1 states left on queue.\nThe depth of the complete state graph search is 1.\nFinished in ";
    assert!(out.contains(want), "{out}");
    // An initial state whose VIEW fails counts as generated.
    let (out, code) = run(&["EK2.tla"]);
    assert_eq!(code, 75, "{out}");
    assert!(out.contains("\n2 states generated, 1 distinct states found, 1 states left on queue.\n"), "{out}");
    // So it does when the VIEW is compiled (its encoded fingerprint fails;
    // a one-state pre-run does not see the failing state).
    let (out, code) = run_env(&["EK2.tla", "-compile"], &[("TLX_PRERUN", "1")]);
    assert_eq!(code, 75, "{out}");
    assert!(out.contains("\n2 states generated, 1 distinct states found, 1 states left on queue.\n"), "{out}");
    // A general error while checking an initial state shows the state, and
    // no behavior.
    let (out, code) = run(&["EK3.tla"]);
    assert_eq!(code, 75, "{out}");
    assert!(
        out.contains(
            "\nWhile working on the initial state:\nx = 0\n\n1 states generated, 1 distinct states found, 1 states left on queue.\n"
        ),
        "{out}"
    );
    assert!(!out.contains("The behavior up to this point"), "{out}");
    // One of a specific kind is that kind's message, nested in a general one.
    let (out, code) = run(&["EK3.tla", "-config", "EK3b.cfg", "-tool"]);
    assert_eq!(code, 75, "{out}");
    let want = "@!@!@STARTMSG 1000:1 @!@!@\n@!@!@STARTMSG 2179:0 @!@!@\nThe second argument of \\div is 0.\n@!@!@ENDMSG 2179 @!@!@\n@!@!@ENDMSG 1000 @!@!@\n@!@!@STARTMSG 2200:0 @!@!@\nProgress(1) at ";
    assert!(out.contains(want), "{out}");
    // An integer literal TLC cannot represent: rejected at the start.
    let (out, code) = run(&["C2.tla"]);
    assert_eq!(code, 255, "{out}");
    assert!(out.contains("Error: TLC can't handle a number this big.\n3000000000\nFinished in "), "{out}");
    assert!(!out.contains("Computing initial states"), "{out}");
}

#[test]
fn next_state_error_kinds() {
    // A general error: an unexpected exception (exit 75).
    let (out, code) = run(&["EK4.tla"]);
    assert_eq!(code, 75, "{out}");
    assert!(out.contains(&format!("{UNEXPECTED}java.lang.RuntimeException\n: Attempted ")), "{out}");
    // Assert: its own message, without a location (exit 14).
    let (out, code) = run(&["EK4.tla", "-config", "EK4b.cfg"]);
    assert_eq!(code, 14, "{out}");
    let want = "Error: The first argument of Assert evaluated to FALSE; the second argument was:\n\"boom\"\nError: The behavior up to this point is:\n";
    assert!(out.contains(want), "{out}");
    // Division by zero: TLC_MODULE_DIVISION_BY_ZERO (exit 255).
    let (out, code) = run(&["EK4.tla", "-config", "EK4c.cfg", "-tool"]);
    assert_eq!(code, 255, "{out}");
    let want = "@!@!@STARTMSG 2179:1 @!@!@\nThe second argument of \\div is 0.\n@!@!@ENDMSG 2179 @!@!@\n@!@!@STARTMSG 2121:1 @!@!@\n";
    assert!(out.contains(want), "{out}");
    // An invariant that cannot be evaluated (exit 76), with the outdegree.
    let (out, code) = run(&["EK4.tla", "-config", "EK4d.cfg"]);
    assert_eq!(code, 76, "{out}");
    let want = "Error: Evaluating invariant Inv failed.\nThe second argument of \\div is 0.\nError: The behavior up to this point is:\n";
    assert!(out.contains(want), "{out}");
    assert!(out.contains("The average outdegree of the complete state graph is "), "{out}");
}

#[test]
fn simulation_errors() {
    // While computing the initial states: an unexpected exception, then the
    // number of states generated (exit 255).
    let (out, code) = run(&["EK1.tla", "-simulate", "num=2"]);
    assert_eq!(code, 255, "{out}");
    let want = format!("{UNEXPECTED}tlc2.tool.EvalException\n: The second argument of \\div is 0.\nThe number of states generated: 0\nSimulation using seed ");
    assert!(out.contains(&want), "{out}");
    // While checking an initial state.
    let (out, code) = run(&["EK3.tla", "-simulate", "num=2"]);
    assert_eq!(code, 255, "{out}");
    assert!(out.contains("\nWhile working on the initial state:\nx = 0\n\nThe number of states generated: 1\n"), "{out}");
    // In an action, with -tool: the kind's message nested in the exception's.
    let (out, code) = run(&["EK4.tla", "-config", "EK4c.cfg", "-simulate", "num=1", "-tool"]);
    assert_eq!(code, 255, "{out}");
    let want = "tlc2.tool.EvalException\n: @!@!@STARTMSG 2179:0 @!@!@\nThe second argument of \\div is 0.\n@!@!@ENDMSG 2179 @!@!@\n@!@!@ENDMSG 1000 @!@!@\n";
    assert!(out.contains(want), "{out}");
    // In a predicate of a temporal property, checked on the behavior: the
    // behavior follows.
    let (out, code) = run(&["LF5.tla", "-simulate", "num=2", "-seed", "1", "-deadlock"]);
    assert_eq!(code, 255, "{out}");
    let want = ": The second argument of \\div is 0.\nError: The behavior up to this point is:\nState 1: <Init line 7, col 9 to line 7, col 13 of module LF5>\nx = 0\n\n";
    assert!(out.contains(want) && out.contains("State 5: <Next line 8, col 9 to line 8, col 27 of module LF5>\nx = 4\n"), "{out}");
}

#[test]
fn generation() {
    // -generate: one state per step (G1's POSTCONDITION checks it).
    let (out, code) = run(&["G1.tla", "-generate", "num=20", "-depth", "5"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("The number of states generated: 101\n"), "{out}");
    // Its ASSUME requires TLCGet("config").mode = "generate".
    let (out, code) = run(&["G1.tla", "-simulate", "num=20", "-depth", "5"]);
    assert_eq!(code, 10, "{out}");
    // CASE is not supported in actions.
    let (out, code) = run(&["G2.tla", "-generate", "num=3", "-depth", "3"]);
    assert_eq!(code, 255, "{out}");
    let want = format!("{UNEXPECTED}java.lang.UnsupportedOperationException\n: Probabilistic evaluation of next-state relation not implemented for CASE yet.\n");
    assert!(out.contains(&want), "{out}");
    // -depth -1: unbounded behaviors (S5's end at its constraints).
    let (out, code) = run(&["S5.tla", "-simulate", "num=3", "-depth", "-1"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("The number of states generated: 14\n"), "{out}");
}

#[test]
fn csv_and_io_natives() {
    // CSVWrite / CSVRecords (CSV) and IOExec (IOUtils) as TLC's overrides;
    // files are relative to the working directory.
    let Some(csv) = tlx_testkit::require_third_party("csv_and_io_natives", "community/CSV.tla") else {
        return;
    };
    let community = csv.parent().unwrap().to_path_buf();
    let dir = tlx_testkit::TempDir::new("output");
    let spec = [
        "---- MODULE CSVT ----",
        "EXTENDS TLC, CSV, IOUtils",
        "VARIABLE x",
        "ASSUME CSVRecords(\"t.csv\") = 0",
        "ASSUME CSVWrite(\"%1$s#%2$s\", <<\"a\", 42>>, \"t.csv\") /\\ CSVWrite(\"%2$s\", <<1, {2}>>, \"t.csv\")",
        "ASSUME CSVRecords(\"t.csv\") = 2",
        "ASSUME IOExec(<<\"sh\", \"-c\", \"echo hi; exit 3\">>) = [exitValue |-> 3, stdout |-> \"hi\\n\", stderr |-> \"\"]",
        "Init == x = 0",
        "Next == UNCHANGED x",
        "====",
    ];
    std::fs::write(dir.join("CSVT.tla"), spec.join("\n")).unwrap();
    std::fs::write(dir.join("CSVT.cfg"), "INIT Init\nNEXT Next\n").unwrap();
    let out = tlx_testkit::tlx!().args(["check", "CSVT.tla", "-I"]).arg(&community).dir(&dir).run();
    let text = out.stdout;
    let csv = std::fs::read_to_string(dir.join("t.csv")).unwrap_or_default();
    assert_eq!(out.code, 0, "{text}");
    assert_eq!(csv, "\"a\"#42\n{2}\n");
}

#[test]
fn liveness_messages() {
    // TLC announces the branches of the temporal properties.
    let (out, _) = run(&["L7.tla"]);
    assert!(out.contains("Implied-temporal checking--satisfiability problem has 3 branches.\nComputing initial states...\n"), "{out}");
    // []<>P and <>[]P conjuncts take no branch of their own (TLC 2026.10).
    let (out, _) = run(&["MX4.tla"]);
    assert!(out.contains("satisfiability problem has 2 branches.\n"), "{out}");
    assert!(out.contains("Checking 2 branches of temporal properties for the complete state space with 6 total distinct states"), "{out}");
    // Constant subformulas are folded before counting (TLC: 1 branch, not 2).
    let (out, _) = run(&["LF9.tla"]);
    assert!(out.contains("satisfiability problem has 1 branches.\n"), "{out}");
    // VIEW: no warning; SYMMETRY: TLC's warning, after "Starting...".
    let (out, code) = run(&["V4.tla"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("Warning"), "{out}");
    let (out, code) = run(&["V5.tla", "-tool"]);
    assert_eq!(code, 0, "{out}");
    let (start, warning) =
        (out.find("Starting... ("), out.find("@!@!@STARTMSG 2279:3 @!@!@\nDeclaring symmetry during liveness checking is dangerous."));
    assert!(start.is_some() && warning.is_some() && start < warning, "{out}");
}

#[test]
fn compiled_print_output() {
    // PrintT in an action prints once per evaluation of the search: not
    // again in the compiled tier's shape pre-run.
    let prints = |extra: &[&str]| {
        let out = tlx_testkit::tlx!().args(["check", "MX2.tla"]).args(extra).dir(specs()).run();
        let text = out.text();
        let mut lines: Vec<String> = text.lines().filter(|l| l.starts_with("<<\"inc\"")).map(String::from).collect();
        lines.sort();
        lines
    };
    let want: Vec<String> = (0..3).flat_map(|x| (0..2).map(move |y| format!("<<\"inc\", {x}, {y}>>"))).collect();
    assert_eq!(prints(&[]), want);
    assert_eq!(prints(&["-compile"]), want);
}

#[test]
fn incomplete_states() {
    // An initial state without a value for y: not a "legal state" (exit
    // 255), shown with null, without statistics.
    for extra in [&[][..], &["-compile"]] {
        let (out, code) = run(&[&["IS1.tla"][..], extra].concat());
        assert_eq!(code, 255, "{extra:?} {out}");
        let want =
            "\nError: current state is not a legal state\nWhile working on the initial state:\n/\\ x = 0\n/\\ y = null\n\nFinished in ";
        assert!(out.contains(want), "{extra:?} {out}");
    }
    let (out, code) = run(&["IS1.tla", "-tool"]);
    assert_eq!(code, 255, "{out}");
    assert!(out.contains("@!@!@STARTMSG 2102:1 @!@!@\ncurrent state is not a legal state\n"), "{out}");
    // In simulation: all initial states are counted first.
    let (out, code) = run(&["IS1.tla", "-simulate", "num=2", "-tool"]);
    assert_eq!(code, 255, "{out}");
    let want = "Computed 2 initial states...\n@!@!@ENDMSG 2269 @!@!@\n@!@!@STARTMSG 2106:1 @!@!@\nState is not completely specified by the initial predicate:\n/\\ x = 0\n/\\ y = null\n\n@!@!@ENDMSG 2106 @!@!@\n@!@!@STARTMSG 2186:0 @!@!@\n";
    assert!(out.contains(want), "{out}");
    // A successor without values for y and z, by one of several actions
    // (named): the behavior ends with it (exit 75).
    for extra in [&[][..], &["-compile"], &["-simulate", "num=1", "-seed", "1"]] {
        let (out, code) = run(&[&["IS2.tla"][..], extra].concat());
        assert_eq!(code, 75, "{extra:?} {out}");
        let want = "Error: Successor state is not completely specified by action B of the next-state relation. The following variables are not defined: y, z.\n\nError: The behavior up to this point is:\n";
        assert!(out.contains(want), "{extra:?} {out}");
        assert!(
            out.contains("\nState 4: <B line 8, col 6 to line 8, col 24 of module IS2>\n/\\ x = 3\n/\\ y = null\n/\\ z = null\n\n"),
            "{extra:?} {out}"
        );
    }
}

#[test]
fn subscript_warning() {
    // TLC's warning for the variables the subscript of [][Next]_v does not
    // contain, in the order TLC prints variables, after "Starting...".
    let (out, code) = run(&["SW1.tla"]);
    assert_eq!(code, 0, "{out}");
    let want = "Warning: The subscript of the next-state relation specified by the specification\ndoes not seem to contain the state variable w\n(Use the -nowarning option to disable this warning.)\nWarning: The subscript of the next-state relation specified by the specification\ndoes not seem to contain the state variable y\nComputing initial states...\n";
    assert!(out.contains(want), "{out}");
    let (out, _) = run(&["SW1.tla", "-tool"]);
    assert!(out.contains("@!@!@STARTMSG 2140:3 @!@!@\nThe subscript of the next-state relation"), "{out}");
    let (out, _) = run(&["SW1.tla", "-nowarning"]);
    assert!(!out.contains("Warning"), "{out}");
    // Not for INIT / NEXT, nor for a complete subscript.
    for spec in ["T2.tla", "L1.tla", "V4.tla"] {
        let (out, _) = run(&[spec]);
        assert!(!out.contains("does not seem to contain"), "{spec}: {out}");
    }
}

#[test]
fn evaluation_failures_of_assumptions_and_postconditions() {
    // An assumption that fails to evaluate (exit 10).
    let (out, code) = run(&["AS1.tla"]);
    assert_eq!(code, 10, "{out}");
    let want = "Error: Evaluating assumption line 5, col 8 to line 5, col 19 of module AS1 failed.\nThe second argument of \\div is 0.\nFinished in ";
    assert!(out.contains(want), "{out}");
    // A postcondition that fails to evaluate (exit 10); the kind's message
    // nested with -tool.
    for extra in [&[][..], &["-compile"]] {
        let (out, code) = run(&[&["M4.tla"][..], extra].concat());
        assert_eq!(code, 10, "{extra:?} {out}");
        let want = "Error: Evaluating postcondition Post at line 8, col 9 to line 8, col 54 of module M4 failed.\nThe first argument of Assert evaluated to FALSE; the second argument was:\n<<\"count\", 3>>\n4 states generated, ";
        assert!(out.contains(want), "{extra:?} {out}");
    }
    let (out, code) = run(&["M4.tla", "-tool"]);
    assert_eq!(code, 10, "{out}");
    let want = "@!@!@STARTMSG 2405:1 @!@!@\nEvaluating postcondition Post at line 8, col 9 to line 8, col 54 of module M4 failed.\n@!@!@STARTMSG 2132:0 @!@!@\nThe first argument of Assert evaluated to FALSE; the second argument was:\n<<\"count\", 3>>\n@!@!@ENDMSG 2132 @!@!@\n@!@!@ENDMSG 2405 @!@!@\n";
    assert!(out.contains(want), "{out}");
}

#[test]
fn warns_about_flags_without_effect() {
    // -dump does nothing with a simulation.
    let dump = std::env::temp_dir().join(format!("tlx-ignored-{}.dump", std::process::id()));
    let (out, code) = run(&["S5.tla", "-simulate", "num=1", "-seed", "1", "-depth", "10", "-dump", dump.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Warning: -dump has no effect with -simulate or -generate."), "{out}");
    // -seed without -simulate is not warned about: it still seeds
    // RandomElement / Randomization, as in TLC.
    let (out, code) = run(&["T2.tla", "-seed", "1"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("-seed"), "{out}");
    let (out, code) = run(&["T2.tla", "-depth", "3"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Warning: -depth has no effect without -simulate or -generate."), "{out}");
    // Nothing when they take effect, or when they are not given.
    let (out, code) = run(&["S5.tla", "-simulate", "num=1", "-seed", "1", "-depth", "10"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("has no effect"), "{out}");
    let (out, _) = run(&["T2.tla"]);
    assert!(!out.contains("has no effect"), "{out}");
}

#[test]
fn tool_diagnostics_go_to_stderr() {
    // With -tool, stdout carries only TLC's framed messages: the compile
    // diagnostics go to stderr instead.
    let tlx = |extra: &[&str]| {
        let out = tlx_testkit::tlx!().args(["check", "M3.tla", "-compile"]).args(extra).env("TLX_PRERUN", "5").dir(specs()).run();
        (out.code, out.stdout, out.stderr)
    };
    let (code, out, err) = tlx(&["-tool"]);
    assert_eq!(code, 0, "{out} {err}");
    assert!(err.contains("compile: "), "{err}");
    assert!(!out.contains("compile: "), "{out}");
    let (code, out, err) = tlx(&[]);
    assert_eq!(code, 0, "{out} {err}");
    assert!(!err.contains("compile: "), "{err}");
    assert!(out.contains("compile: "), "{out}");
}

/// A closed stdout (`tlx check Spec.tla | head -3`) must not panic. The
/// pipe's read end is closed before tlx is spawned, so its very first write
/// hits a broken pipe whatever the scheduling (closing it after the spawn
/// races with a child that already finished writing).
#[test]
#[cfg(unix)]
fn closed_stdout_exits_quietly() {
    use std::os::fd::FromRawFd;
    use std::os::unix::process::ExitStatusExt;
    use std::process::Stdio;
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: fds is aligned, initialized and writable for exactly the two
    // descriptors pipe returns. Check success before acquiring their ownership.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe");
    // SAFETY: both descriptors are fresh and owned here; the write end moves into `Stdio`.
    let (write_end, read_end) =
        unsafe { (Stdio::from(std::os::fd::OwnedFd::from_raw_fd(fds[1])), std::os::fd::OwnedFd::from_raw_fd(fds[0])) };
    drop(read_end);
    let out = tlx_testkit::tlx!()
        .args(["check", "T4.tla"])
        .dir(specs())
        .command()
        .stdout(write_end)
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tlx")
        .wait_with_output()
        .expect("wait for tlx");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("panicked") && !err.contains("Broken pipe"), "{err}");
    assert_eq!(out.status.signal(), Some(libc::SIGPIPE), "{err}");
}

/// `run` with environment variables.
fn run_env(args: &[&str], env: &[(&str, &str)]) -> (String, i32) {
    let out = tlx_testkit::tlx!().arg("check").args(args).envs(env.iter().copied()).dir(specs()).run();
    (out.stdout, out.code)
}

#[test]
fn postcondition_reads_worker_0_registers() {
    // M3's Next counts its evaluations in register 1; the POSTCONDITION
    // reads worker 0's. The compiled tier's pre-run and restarts (a small
    // pre-run misses the shapes of x and y) must not count.
    let cases: [(&[&str], &[(&str, &str)]); 3] = [(&[], &[]), (&["-compile"], &[]), (&["-compile"], &[("TLX_PRERUN", "5")])];
    for (extra, env) in cases {
        let (out, code) = run_env(&[&["M3.tla"][..], extra].concat(), env);
        assert_eq!(code, 0, "{extra:?} {env:?} {out}");
        assert!(out.contains("51 states generated, 51 distinct states found"), "{extra:?} {env:?} {out}");
        if !env.is_empty() {
            assert!(out.contains("compile: layout miss on "), "{extra:?} {env:?} {out}");
        }
    }
    // In simulation (G3's CONSTRAINT counts the states).
    let cases: [(&[&str], &[(&str, &str)]); 2] = [(&[], &[]), (&["-compile"], &[("TLX_PRERUN", "2"), ("TLX_PRERUN_MS", "1")])];
    for (extra, env) in cases {
        let (out, code) = run_env(&[&["G3.tla", "-simulate", "num=4", "-depth", "40"][..], extra].concat(), env);
        assert_eq!(code, 0, "{extra:?} {env:?} {out}");
    }
}
