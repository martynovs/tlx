//! `-dumpTrace json|tla FILE`: the counterexample files are byte-identical
//! to TLC's (expected texts taken from TLC 2026.10.02 with
//! `-dumpTrace json t.json -dumpTrace tla tt.tla`), in every tier: safety
//! counterexamples and liveness lassos.

const SPEC: &str = r#"---- MODULE E ----
EXTENDS Naturals
CONSTANT Mode
VARIABLES y, x
Init == x = 0 /\ y = [b |-> {2, 1}, a |-> "s"]
Step(k, m) == x < 2 /\ x' = x + k + m /\ y' = y
Next == \E k \in {1} : \E m \in {0} : Step(k, m)
Inv == IF Mode = "init" THEN x > 0 ELSE IF Mode = "err" THEN (x < 2 \/ 1 \div 0 = 1) ELSE TRUE
Live == <>(x = 5)
Spec == Init /\ [][Next]_<<x, y>> /\ WF_<<x, y>>(Next)
====
"#;

const DEADLOCK_JSON: &str = r#"{"counterexample":{"action":[[[1,{"x":0,"y":{"b":[1,2],"a":"s"}}],{"name":"Step","location":{"beginLine":6,"beginColumn":15,"endLine":6,"endColumn":47,"module":"E"},"context":{"k":1,"m":0},"parameters":["k","m"]},[2,{"x":1,"y":{"b":[1,2],"a":"s"}}]],[[2,{"x":1,"y":{"b":[1,2],"a":"s"}}],{"name":"Step","location":{"beginLine":6,"beginColumn":15,"endLine":6,"endColumn":47,"module":"E"},"context":{"k":1,"m":0},"parameters":["k","m"]},[3,{"x":2,"y":{"b":[1,2],"a":"s"}}]]],"state":[[1,{"x":0,"y":{"b":[1,2],"a":"s"}}],[2,{"x":1,"y":{"b":[1,2],"a":"s"}}],[3,{"x":2,"y":{"b":[1,2],"a":"s"}}]]},"vars":["y","x"]}"#;

const DEADLOCK_TLA: &str = "---- MODULE tt ----\nLOCAL INSTANCE TLC\n\nCounterExample == \n\t[action |-> {<<<<1, [x |-> 0, y |-> [b |-> {1, 2}, a |-> \"s\"]]>>, [name |-> \"Step\", location |-> [beginLine |-> 6, beginColumn |-> 15, endLine |-> 6, endColumn |-> 47, module |-> \"E\"], context |-> [k |-> 1, m |-> 0], parameters |-> <<\"k\", \"m\">>], <<2, [x |-> 1, y |-> [b |-> {1, 2}, a |-> \"s\"]]>>>>, <<<<2, [x |-> 1, y |-> [b |-> {1, 2}, a |-> \"s\"]]>>, [name |-> \"Step\", location |-> [beginLine |-> 6, beginColumn |-> 15, endLine |-> 6, endColumn |-> 47, module |-> \"E\"], context |-> [k |-> 1, m |-> 0], parameters |-> <<\"k\", \"m\">>], <<3, [x |-> 2, y |-> [b |-> {1, 2}, a |-> \"s\"]]>>>>}, state |-> {<<1, [x |-> 0, y |-> [b |-> {1, 2}, a |-> \"s\"]]>>, <<2, [x |-> 1, y |-> [b |-> {1, 2}, a |-> \"s\"]]>>, <<3, [x |-> 2, y |-> [b |-> {1, 2}, a |-> \"s\"]]>>}]\n\n====";

const INIT_JSON: &str = r#"{"counterexample":{"action":[],"state":[[1,{"x":0,"y":{"b":[1,2],"a":"s"}}]]},"vars":["y","x"]}"#;

const INIT_TLA: &str = "---- MODULE tt ----\nLOCAL INSTANCE TLC\n\nCounterExample == \n\t[action |-> {}, state |-> {<<1, [x |-> 0, y |-> [b |-> {1, 2}, a |-> \"s\"]]>>}]\n\n====";

/// Run E in a fresh directory with `Mode = mode` and `cfg` (plus the tlx
/// arguments `args`): (exit code, output, json file, tla file).
fn run(mode: &str, cfg: &str, args: &[&str], env: &[(&str, &str)]) -> (i32, String, Option<String>, Option<String>) {
    let dir = tlx_testkit::TempDir::new("dump_trace");
    std::fs::write(dir.join("E.tla"), SPEC).unwrap();
    std::fs::write(dir.join("E.cfg"), format!("CONSTANT Mode = \"{mode}\"\nSPECIFICATION Spec\n{cfg}\n")).unwrap();
    let mut cmd = tlx_testkit::tlx!();
    cmd.args(["check", "E.tla", "-workers", "1", "-dumpTrace", "json", "t.json", "-dumpTrace", "tla", "tt.tla"]).args(args).dir(&dir);
    let out = cmd.envs(env.iter().copied()).run();
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok();
    (out.code, out.text(), read("t.json"), read("tt.tla"))
}

use tlx_testkit::cli::TIERS;

#[test]
fn deadlock_counterexample_as_tlc() {
    for (_, args, env) in TIERS.iter().copied() {
        let (code, out, json, tla) = run("dead", "", args, env);
        assert_eq!(code, 11, "{args:?}: {out}");
        assert_eq!(json.as_deref(), Some(DEADLOCK_JSON), "{args:?}: {out}");
        assert_eq!(tla.as_deref(), Some(DEADLOCK_TLA), "{args:?}: {out}");
        assert!(out.contains("\"CounterExample written: t.json\"") && out.contains("\"CounterExample written: tt.tla\""), "{out}");
    }
}

#[test]
fn initial_state_counterexample_as_tlc() {
    let (code, out, json, tla) = run("init", "INVARIANT Inv", &[], &[]);
    assert_eq!(code, 12, "{out}");
    assert_eq!(json.as_deref(), Some(INIT_JSON), "{out}");
    assert_eq!(tla.as_deref(), Some(INIT_TLA), "{out}");
}

#[test]
fn no_counterexample_after_an_error() {
    // TLC writes nothing after an evaluation error (nor without a violation).
    let (_, out, json, tla) = run("err", "INVARIANT Inv", &[], &[]);
    assert!(json.is_none() && tla.is_none(), "{out}");
    let (code, out, json, tla) = run("ok", "CHECK_DEADLOCK FALSE", &[], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(json.is_none() && tla.is_none(), "{out}");
}

// Liveness counterexamples: TLC's lasso has one more step, back to the
// state the cycle starts with (expected texts from TLC 2026.10.02 with
// `-dumpTrace json t.json -dumpTrace tla tt.tla`; liveness traces are
// tier 3, these lassos are the only ones of their specs).

/// Run module `name` (text `spec`, cfg `SPECIFICATION Spec`, `PROPERTY
/// Prop`, no deadlock) in a fresh directory (plus the tlx arguments
/// `args`): (exit code, output, json file, tla file).
fn run_live(name: &str, spec: &str, args: &[&str], env: &[(&str, &str)]) -> (i32, String, Option<String>, Option<String>) {
    let dir = tlx_testkit::TempDir::new("dump_trace");
    std::fs::write(dir.join(format!("{name}.tla")), spec).unwrap();
    std::fs::write(dir.join(format!("{name}.cfg")), "SPECIFICATION Spec\nPROPERTY Prop\nCHECK_DEADLOCK FALSE\n").unwrap();
    let mut cmd = tlx_testkit::tlx!();
    cmd.args(["check", &format!("{name}.tla"), "-dumpTrace", "json", "t.json", "-dumpTrace", "tla", "tt.tla"]).args(args).dir(&dir);
    let out = cmd.envs(env.iter().copied()).run();
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok();
    (out.code, out.text(), read("t.json"), read("tt.tla"))
}

/// The tla file TLC writes for `CounterExample == text`.
fn tla_file(text: &str) -> String {
    format!("---- MODULE tt ----\nLOCAL INSTANCE TLC\n\nCounterExample == \n\t{text}\n\n====")
}

/// `name` violates its property with the lasso `json` / `tla` in every tier.
fn check_lasso(name: &str, spec: &str, json: &str, tla: &str) {
    for (_, args, env) in TIERS.iter().copied().chain([("parallel", &["-workers", "3"][..], &[][..])]) {
        let (code, out, j, t) = run_live(name, spec, args, env);
        assert_eq!(code, 13, "{args:?}: {out}");
        assert_eq!(j.as_deref(), Some(json), "{args:?}: {out}");
        assert_eq!(t.as_deref(), Some(tla_file(tla).as_str()), "{args:?}: {out}");
        assert!(out.contains("\"CounterExample written: t.json\"") && out.contains("\"CounterExample written: tt.tla\""), "{out}");
    }
}

const L1: &str = r#"---- MODULE L1 ----
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = "a"
Inc(d) == x < 3 /\ x' = x + d /\ UNCHANGED y
Reset == x = 3 /\ x' = 1 /\ y' = "b"
Next == (\E d \in {1} : Inc(d)) \/ Reset
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
Prop == []<>(x = 0)
====
"#;

#[test]
fn lasso_back_to_a_state_as_tlc() {
    let json = r#"{"counterexample":{"action":[[[1,{"x":0,"y":"a"}],{"name":"Inc","location":{"beginLine":6,"beginColumn":11,"endLine":6,"endColumn":44,"module":"L1"},"context":{"d":1},"parameters":["d"]},[2,{"x":1,"y":"a"}]],[[2,{"x":1,"y":"a"}],{"name":"Inc","location":{"beginLine":6,"beginColumn":11,"endLine":6,"endColumn":44,"module":"L1"},"context":{"d":1},"parameters":["d"]},[3,{"x":2,"y":"a"}]],[[3,{"x":2,"y":"a"}],{"name":"Inc","location":{"beginLine":6,"beginColumn":11,"endLine":6,"endColumn":44,"module":"L1"},"context":{"d":1},"parameters":["d"]},[4,{"x":3,"y":"a"}]],[[4,{"x":3,"y":"a"}],{"name":"Reset","location":{"beginLine":7,"beginColumn":10,"endLine":7,"endColumn":36,"module":"L1"}},[5,{"x":1,"y":"b"}]],[[5,{"x":1,"y":"b"}],{"name":"Inc","location":{"beginLine":6,"beginColumn":11,"endLine":6,"endColumn":44,"module":"L1"},"context":{"d":1},"parameters":["d"]},[6,{"x":2,"y":"b"}]],[[6,{"x":2,"y":"b"}],{"name":"Inc","location":{"beginLine":6,"beginColumn":11,"endLine":6,"endColumn":44,"module":"L1"},"context":{"d":1},"parameters":["d"]},[7,{"x":3,"y":"b"}]],[[7,{"x":3,"y":"b"}],{"name":"Reset","location":{"beginLine":7,"beginColumn":10,"endLine":7,"endColumn":36,"module":"L1"}},[5,{"x":1,"y":"b"}]]],"state":[[1,{"x":0,"y":"a"}],[2,{"x":1,"y":"a"}],[3,{"x":2,"y":"a"}],[4,{"x":3,"y":"a"}],[5,{"x":1,"y":"b"}],[6,{"x":2,"y":"b"}],[7,{"x":3,"y":"b"}]]},"vars":["x","y"]}"#;
    let tla = r#"[action |-> {<<<<1, [x |-> 0, y |-> "a"]>>, [name |-> "Inc", location |-> [beginLine |-> 6, beginColumn |-> 11, endLine |-> 6, endColumn |-> 44, module |-> "L1"], context |-> [d |-> 1], parameters |-> <<"d">>], <<2, [x |-> 1, y |-> "a"]>>>>, <<<<2, [x |-> 1, y |-> "a"]>>, [name |-> "Inc", location |-> [beginLine |-> 6, beginColumn |-> 11, endLine |-> 6, endColumn |-> 44, module |-> "L1"], context |-> [d |-> 1], parameters |-> <<"d">>], <<3, [x |-> 2, y |-> "a"]>>>>, <<<<3, [x |-> 2, y |-> "a"]>>, [name |-> "Inc", location |-> [beginLine |-> 6, beginColumn |-> 11, endLine |-> 6, endColumn |-> 44, module |-> "L1"], context |-> [d |-> 1], parameters |-> <<"d">>], <<4, [x |-> 3, y |-> "a"]>>>>, <<<<4, [x |-> 3, y |-> "a"]>>, [name |-> "Reset", location |-> [beginLine |-> 7, beginColumn |-> 10, endLine |-> 7, endColumn |-> 36, module |-> "L1"]], <<5, [x |-> 1, y |-> "b"]>>>>, <<<<5, [x |-> 1, y |-> "b"]>>, [name |-> "Inc", location |-> [beginLine |-> 6, beginColumn |-> 11, endLine |-> 6, endColumn |-> 44, module |-> "L1"], context |-> [d |-> 1], parameters |-> <<"d">>], <<6, [x |-> 2, y |-> "b"]>>>>, <<<<6, [x |-> 2, y |-> "b"]>>, [name |-> "Inc", location |-> [beginLine |-> 6, beginColumn |-> 11, endLine |-> 6, endColumn |-> 44, module |-> "L1"], context |-> [d |-> 1], parameters |-> <<"d">>], <<7, [x |-> 3, y |-> "b"]>>>>, <<<<7, [x |-> 3, y |-> "b"]>>, [name |-> "Reset", location |-> [beginLine |-> 7, beginColumn |-> 10, endLine |-> 7, endColumn |-> 36, module |-> "L1"]], <<5, [x |-> 1, y |-> "b"]>>>>}, state |-> {<<1, [x |-> 0, y |-> "a"]>>, <<2, [x |-> 1, y |-> "a"]>>, <<3, [x |-> 2, y |-> "a"]>>, <<4, [x |-> 3, y |-> "a"]>>, <<5, [x |-> 1, y |-> "b"]>>, <<6, [x |-> 2, y |-> "b"]>>, <<7, [x |-> 3, y |-> "b"]>>}]"#;
    check_lasso("L1", L1, json, tla);
}

#[test]
fn stuttering_lasso_as_tlc() {
    // The closing step stutters, with the action of the step into the last
    // state (TLC: the last state's action).
    let spec = L1.replace("MODULE L1", "MODULE L2").replace(" /\\ WF_vars(Next)", "").replace("[]<>(x = 0)", "<>(x = 5)");
    let json = r#"{"counterexample":{"action":[[[1,{"x":0,"y":"a"}],{"name":"Inc","location":{"beginLine":6,"beginColumn":11,"endLine":6,"endColumn":44,"module":"L2"},"context":{"d":1},"parameters":["d"]},[2,{"x":1,"y":"a"}]],[[2,{"x":1,"y":"a"}],{"name":"Inc","location":{"beginLine":6,"beginColumn":11,"endLine":6,"endColumn":44,"module":"L2"},"context":{"d":1},"parameters":["d"]},[3,{"x":2,"y":"a"}]],[[3,{"x":2,"y":"a"}],{"name":"Inc","location":{"beginLine":6,"beginColumn":11,"endLine":6,"endColumn":44,"module":"L2"},"context":{"d":1},"parameters":["d"]},[4,{"x":3,"y":"a"}]],[[4,{"x":3,"y":"a"}],{"name":"Reset","location":{"beginLine":7,"beginColumn":10,"endLine":7,"endColumn":36,"module":"L2"}},[5,{"x":1,"y":"b"}]],[[5,{"x":1,"y":"b"}],{"name":"Reset","location":{"beginLine":7,"beginColumn":10,"endLine":7,"endColumn":36,"module":"L2"}},[5,{"x":1,"y":"b"}]]],"state":[[1,{"x":0,"y":"a"}],[2,{"x":1,"y":"a"}],[3,{"x":2,"y":"a"}],[4,{"x":3,"y":"a"}],[5,{"x":1,"y":"b"}]]},"vars":["x","y"]}"#;
    let tla = r#"[action |-> {<<<<1, [x |-> 0, y |-> "a"]>>, [name |-> "Inc", location |-> [beginLine |-> 6, beginColumn |-> 11, endLine |-> 6, endColumn |-> 44, module |-> "L2"], context |-> [d |-> 1], parameters |-> <<"d">>], <<2, [x |-> 1, y |-> "a"]>>>>, <<<<2, [x |-> 1, y |-> "a"]>>, [name |-> "Inc", location |-> [beginLine |-> 6, beginColumn |-> 11, endLine |-> 6, endColumn |-> 44, module |-> "L2"], context |-> [d |-> 1], parameters |-> <<"d">>], <<3, [x |-> 2, y |-> "a"]>>>>, <<<<3, [x |-> 2, y |-> "a"]>>, [name |-> "Inc", location |-> [beginLine |-> 6, beginColumn |-> 11, endLine |-> 6, endColumn |-> 44, module |-> "L2"], context |-> [d |-> 1], parameters |-> <<"d">>], <<4, [x |-> 3, y |-> "a"]>>>>, <<<<4, [x |-> 3, y |-> "a"]>>, [name |-> "Reset", location |-> [beginLine |-> 7, beginColumn |-> 10, endLine |-> 7, endColumn |-> 36, module |-> "L2"]], <<5, [x |-> 1, y |-> "b"]>>>>, <<<<5, [x |-> 1, y |-> "b"]>>, [name |-> "Reset", location |-> [beginLine |-> 7, beginColumn |-> 10, endLine |-> 7, endColumn |-> 36, module |-> "L2"]], <<5, [x |-> 1, y |-> "b"]>>>>}, state |-> {<<1, [x |-> 0, y |-> "a"]>>, <<2, [x |-> 1, y |-> "a"]>>, <<3, [x |-> 2, y |-> "a"]>>, <<4, [x |-> 3, y |-> "a"]>>, <<5, [x |-> 1, y |-> "b"]>>}]"#;
    check_lasso("L2", &spec, json, tla);
}

#[test]
fn stuttering_at_the_initial_state_as_tlc() {
    // The action into an initial state is TLC's UnnamedAction.
    let spec = "---- MODULE L3 ----\nEXTENDS Naturals\nVARIABLES x\nInit == x = 0\nNext == x > 0 /\\ x' = x - 1\nSpec == Init /\\ [][Next]_x\nProp == <>(x = 5)\n====\n";
    let json = r#"{"counterexample":{"action":[[[1,{"x":0}],{"name":"UnnamedAction","location":{"beginLine":0,"beginColumn":0,"endLine":0,"endColumn":0,"module":"--TLA+ BUILTINS--"}},[1,{"x":0}]]],"state":[[1,{"x":0}]]},"vars":["x"]}"#;
    let tla = r#"[action |-> {<<<<1, [x |-> 0]>>, [name |-> "UnnamedAction", location |-> [beginLine |-> 0, beginColumn |-> 0, endLine |-> 0, endColumn |-> 0, module |-> "--TLA+ BUILTINS--"]], <<1, [x |-> 0]>>>>}, state |-> {<<1, [x |-> 0]>>}]"#;
    check_lasso("L3", spec, json, tla);
}
