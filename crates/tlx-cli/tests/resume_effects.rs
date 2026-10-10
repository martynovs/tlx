//! A compiled search that restarts after a layout miss (resumed at the
//! interrupted level, or started over) shows the side effects of an
//! uninterrupted search: each Print/PrintT line as often as the
//! interpreter prints it (with one worker, in the same order), and TLCGet
//! registers that continue rather than restart. `TLX_PRERUN=1` (a pre-run
//! of one state) makes layout misses certain.

use std::path::Path;
use tlx_testkit::cli::tests_specs as specs_dir;

/// Registers and output: an action prints a register that an invariant
/// and the action itself advance.
const REG_PRINT: &str = r#"---- MODULE RegPrint ----
EXTENDS Naturals, TLC
VARIABLES x, y
Init == x = 0 /\ y = 0 /\ TLCSet(1, 0)
Inc == x < 5 /\ TLCSet(1, TLCGet(1) + 1) /\ PrintT(<<"inc", x, y, TLCGet(1)>>) /\ x' = x + 1 /\ y' = y
Flip == y' = 1 - y /\ x' = x
Next == Inc \/ Flip
Inv == TLCSet(1, TLCGet(1) + 10)
====
"#;

/// A simulation (one behavior per run, deterministic) printing every step.
const SIM_PRINT: &str = r#"---- MODULE SimPrint ----
EXTENDS Naturals, TLC
VARIABLE x
Init == x = 0
Next == x < 6 /\ PrintT(<<"step", x>>) /\ x' = x + 1
====
"#;

/// A directory holding `spec` as NAME.tla with `cfg` (one per test).
fn spec_dir(name: &str, spec: &str, cfg: &str) -> tlx_testkit::TempDir {
    let dir = tlx_testkit::TempDir::new("resume_effects");
    std::fs::write(dir.join(format!("{name}.tla")), spec).unwrap();
    std::fs::write(dir.join(format!("{name}.cfg")), cfg).unwrap();
    dir
}

/// Run `tlx check NAME.tla` in `dir` with `env` and `args`: the whole output
/// and the lines printed by PrintT (tuples starting with a string).
fn run(dir: &Path, name: &str, env: &[(&str, &str)], args: &[&str]) -> (String, Vec<String>) {
    let mut cmd = tlx_testkit::tlx!();
    cmd.args(["check", &format!("{name}.tla"), "-config", &format!("{name}.cfg")]).args(args).dir(dir);
    let out = cmd.envs(env.iter().copied()).run();
    let text = out.text();
    assert_eq!(out.code, 0, "tlx {args:?} with {env:?} failed:\n{text}");
    let lines = text.lines().filter(|l| l.starts_with("<<\"")).map(str::to_owned).collect();
    (text, lines)
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

/// MX2 (tests/specs): -compile with forced misses prints what the
/// interpreter prints, resumed or started over, with 1 and 4 workers.
#[test]
fn mx2_prints_once_after_resume() {
    let dir = specs_dir();
    let (_, interp) = run(&dir, "MX2", &[], &[]);
    assert_eq!(interp.len(), 6, "interpreter output:\n{interp:?}");
    let (text, resumed) = run(&dir, "MX2", &[("TLX_PRERUN", "1")], &["-compile"]);
    assert!(text.contains("compile: resuming the search"), "no resumed search:\n{text}");
    assert_eq!(resumed, interp, "resumed -compile:\n{text}");
    let (text, over) = run(&dir, "MX2", &[("TLX_PRERUN", "1"), ("TLX_RETAIN_MB", "0")], &["-compile"]);
    assert!(text.contains("layout miss") && !text.contains("resuming"), "not started over:\n{text}");
    assert_eq!(over, interp, "-compile started over:\n{text}");
    let (text, w4) = run(&dir, "MX2", &[("TLX_PRERUN", "1")], &["-compile", "-workers", "4"]);
    assert!(text.contains("layout miss"), "no layout miss:\n{text}");
    assert_eq!(sorted(w4), sorted(interp), "-compile -workers 4:\n{text}");
}

/// Registers continue across a resumed search: the printed register
/// values (which also count invariant evaluations) are the interpreter's.
#[test]
fn registers_continue_after_resume() {
    let dir = spec_dir("RegPrint", REG_PRINT, "INIT Init\nNEXT Next\nINVARIANT Inv\n");
    let (_, interp) = run(&dir, "RegPrint", &[], &[]);
    let resumed = run(&dir, "RegPrint", &[("TLX_PRERUN", "1")], &["-compile"]);
    let over = run(&dir, "RegPrint", &[("TLX_PRERUN", "1"), ("TLX_RETAIN_MB", "0")], &["-compile"]);
    assert_eq!(interp.len(), 10, "interpreter output:\n{interp:?}");
    assert!(resumed.0.contains("compile: resuming the search"), "no resumed search:\n{}", resumed.0);
    assert_eq!(resumed.1, interp, "resumed -compile:\n{}", resumed.0);
    assert_eq!(over.1, interp, "-compile started over:\n{}", over.0);
}

/// A simulation started over after a layout miss does not repeat its output.
#[test]
fn simulation_prints_once_after_restart() {
    let dir = spec_dir("SimPrint", SIM_PRINT, "INIT Init\nNEXT Next\nCHECK_DEADLOCK FALSE\n");
    let args = ["-simulate", "num=2", "-depth", "10", "-seed", "1"];
    let (_, interp) = run(&dir, "SimPrint", &[], &args);
    let compiled = run(&dir, "SimPrint", &[("TLX_PRERUN", "1"), ("TLX_PRERUN_MS", "0")], &[&["-compile"][..], &args].concat());
    assert_eq!(interp.len(), 12, "interpreter output:\n{interp:?}");
    assert!(compiled.0.contains("layout miss"), "no layout miss:\n{}", compiled.0);
    assert_eq!(compiled.1, interp, "-compile:\n{}", compiled.0);
}

/// A program run by IOExec cannot be undone: a restarted (resumed or
/// started over) search, and the search after the pre-run, get the results
/// of the commands evaluated before instead of running them again.
/// Each run of the command appends a line to `runs.log`: as many lines as
/// with the interpreter (TLC: 10 for the search). (A simulation with
/// IOExec has no pre-run, IOExec making strings, so no layout miss.)
#[test]
fn ioexec_runs_once_after_restart() {
    let Some(io) = tlx_testkit::require_third_party("ioexec_runs_once_after_restart", "community/IOUtils.tla") else {
        return;
    };
    let community = io.parent().unwrap().to_path_buf();
    let spec = r#"---- MODULE IoRun ----
EXTENDS Naturals, TLC, IOUtils
VARIABLES x, y
Init == x = 0 /\ y = 0
Inc == x < 5 /\ IOExec(<<"sh", "-c", "echo r >> runs.log">>).exitValue = 0 /\ x' = x + 1 /\ y' = y
Flip == y' = 1 - y /\ x' = x
Next == Inc \/ Flip
SimNext == x < 6 /\ IOExec(<<"sh", "-c", "echo r >> runs.log">>).exitValue = 0 /\ x' = x + 1 /\ y' = y
====
"#;
    let dir = spec_dir("IoRun", spec, "INIT Init\nNEXT Next\n");
    std::fs::write(dir.join("IoSim.cfg"), "INIT Init\nNEXT SimNext\nCHECK_DEADLOCK FALSE\n").unwrap();
    let runs = |cfg: &str, args: &[&str], env: &[(&str, &str)]| {
        let _ = std::fs::remove_file(dir.join("runs.log"));
        let mut cmd = tlx_testkit::tlx!();
        cmd.args(["check", "IoRun.tla", "-config", cfg, "-I"]).arg(&community).args(args).dir(&dir);
        let out = cmd.envs(env.iter().copied()).run();
        let text = out.text();
        assert_eq!(out.code, 0, "tlx {args:?} with {env:?} failed:\n{text}");
        (std::fs::read_to_string(dir.join("runs.log")).unwrap_or_default().lines().count(), text)
    };
    let miss = [("TLX_PRERUN", "1")];
    let (interp, _) = runs("IoRun.cfg", &[], &[]);
    let (compiled, _) = runs("IoRun.cfg", &["-compile"], &[]);
    let resumed = runs("IoRun.cfg", &["-compile"], &miss);
    let over = runs("IoRun.cfg", &["-compile"], &[("TLX_PRERUN", "1"), ("TLX_RETAIN_MB", "0")]);
    let sim = ["-simulate", "num=2", "-depth", "10", "-seed", "1"];
    let (sim_interp, _) = runs("IoSim.cfg", &sim, &[]);
    let sim_compiled = runs("IoSim.cfg", &[&["-compile"][..], &sim].concat(), &[("TLX_PRERUN", "1"), ("TLX_PRERUN_MS", "0")]);
    assert_eq!(interp, 10);
    assert_eq!(compiled, interp, "-compile (after the pre-run)");
    assert!(resumed.1.contains("compile: resuming the search"), "no resumed search:\n{}", resumed.1);
    assert_eq!(resumed.0, interp, "resumed -compile:\n{}", resumed.1);
    assert!(over.1.contains("layout miss") && !over.1.contains("resuming"), "not started over:\n{}", over.1);
    assert_eq!(over.0, interp, "-compile started over:\n{}", over.1);
    assert_eq!(sim_interp, 12);
    assert_eq!(sim_compiled.0, sim_interp, "simulation -compile:\n{}", sim_compiled.1);
}

/// An action that writes a CSV line and then fails runs in the
/// interpreter only: the line is written as often as by the interpreter
/// alone (the IR executor wrote it, failed, and the interpreter
/// wrote it again).
#[test]
fn failing_action_writes_once() {
    let Some(csv) = tlx_testkit::require_third_party("failing_action_writes_once", "community/CSV.tla") else {
        return;
    };
    let community = csv.parent().unwrap().to_path_buf();
    let spec = r#"---- MODULE CsvFail ----
EXTENDS Integers, TLC, CSV
VARIABLE x
Init == x = 0
Next == /\ x < 4
        /\ CSVWrite("%1$s", <<x>>, "out.csv")
        /\ x' = x + 1
        /\ (x < 2 \/ (CHOOSE n \in {} : TRUE) = 1)
====
"#;
    let dir = spec_dir("CsvFail", spec, "INIT Init\nNEXT Next\n");
    let lines = |args: &[&str], env: &[(&str, &str)]| {
        let _ = std::fs::remove_file(dir.join("out.csv"));
        let mut cmd = tlx_testkit::tlx!();
        cmd.args(["check", "CsvFail.tla", "-I"]).arg(&community).args(args).dir(&dir);
        let out = cmd.envs(env.iter().copied()).run();
        assert_ne!(out.code, 0, "{}", out.stdout);
        std::fs::read_to_string(dir.join("out.csv")).unwrap_or_default()
    };
    let interp = lines(&[], &[]);
    let compiled = lines(&["-compile"], &[("TLX_NATIVE_SYNC", "1")]);
    let ir = lines(&["-compile"], &[("TLX_NO_NATIVE", "1")]);
    assert!(interp.starts_with("0\n1\n2\n"), "interpreter: {interp:?}");
    assert_eq!(compiled, interp, "-compile");
    assert_eq!(ir, interp, "-compile (IR executor)");
}

/// Read before overwriting, then grow a sequence beyond the pre-run's
/// layout. Re-evaluating the interrupted action must read the original
/// bytes, even when restoring only the old length would look plausible.
fn json_overwrite_after_miss(old: &str, original: &str, nd: bool) {
    let spec = r#"---- MODULE JsonOverwrite ----
EXTENDS Naturals, Sequences, TLC, Json
VARIABLES x, q, seen
Init == x = 0 /\ q = <<>> /\ seen = <<>>
Next == /\ x < 3
        /\ seen' = JsonDeserialize("out.json")
        /\ PrintT(<<"read", x, seen'>>)
        /\ JsonSerialize("out.json", <<1>>)
        /\ q' = Append(q, x)
        /\ x' = x + 1
Inv == x = 0 \/ seen = IF x = 1 THEN Original ELSE <<1>>
====
"#;
    let mut spec = spec.replace("Original", original);
    if nd {
        spec = spec.replace("JsonDeserialize", "ndJsonDeserialize").replace("JsonSerialize", "ndJsonSerialize");
    }
    let dir = spec_dir("JsonOverwrite", &spec, "INIT Init\nNEXT Next\nINVARIANT Inv\nCHECK_DEADLOCK FALSE\n");
    let check = |env: &[(&str, &str)], args: &[&str]| {
        std::fs::write(dir.join("out.json"), old).unwrap();
        let out = run(&dir, "JsonOverwrite", env, args);
        let bytes = std::fs::read(dir.join("out.json")).unwrap();
        (out, bytes)
    };
    let (interp, bytes) = check(&[], &[]);
    assert_eq!(interp.1.len(), 3);
    assert_eq!(bytes, if nd { &b"1\n"[..] } else { &b"[1]"[..] });
    for (prerun, retain, paranoid) in [("1", "1024", "0"), ("1", "0", "0"), ("1", "1024", "1"), ("2", "1024", "1")] {
        let (compiled, compiled_bytes) =
            check(&[("TLX_PRERUN", prerun), ("TLX_RETAIN_MB", retain), ("TLX_PARANOID", paranoid)], &["-compile"]);
        if prerun == "1" {
            assert!(compiled.0.contains("layout miss"), "no layout miss:\n{}", compiled.0);
            assert_eq!(compiled.0.contains("compile: resuming the search"), retain != "0", "{}", compiled.0);
        }
        assert_eq!(compiled.1, interp.1, "reads after rollback:\n{}", compiled.0);
        assert_eq!(tlx_testkit::cli::result_lines(&compiled.0), tlx_testkit::cli::result_lines(&interp.0));
        assert_eq!(compiled_bytes, bytes);
    }
}

#[test]
fn shorter_json_overwrite_is_restored_after_layout_miss() {
    json_overwrite_after_miss("[888]", "<<888>>", false);
}

#[test]
fn equal_length_json_overwrite_is_restored_after_layout_miss() {
    json_overwrite_after_miss("[8]", "<<8>>", false);
}

#[test]
fn shorter_ndjson_overwrite_is_restored_after_layout_miss() {
    json_overwrite_after_miss("888\n", "<<888>>", true);
}

#[test]
fn equal_length_ndjson_overwrite_is_restored_after_layout_miss() {
    json_overwrite_after_miss("8\n", "<<8>>", true);
}

/// Serializing a lazy function can evaluate another writer. Its effects
/// must finish before the outer file's checkpoint is captured.
#[test]
fn lazy_json_value_can_write_before_serialization() {
    let spec = r#"---- MODULE LazyJsonWrite ----
EXTENDS Naturals, Sequences, TLC, Json
VARIABLE q
F[i \in 1..2] == IF i = 1 THEN IF JsonSerialize("inner.json", <<Len(q)>>) THEN 1 ELSE 0 ELSE F[i-1] + 1
Init == q = <<>>
Next == /\ Len(q) < 3
        /\ JsonSerialize("out.json", F)
        /\ q' = Append(q, 0)
====
"#;
    for compile in [false, true] {
        let dir = spec_dir("LazyJsonWrite", spec, "INIT Init\nNEXT Next\nCHECK_DEADLOCK FALSE\n");
        let mut cmd = tlx_testkit::tlx!();
        cmd.check("LazyJsonWrite.tla").dir(&dir).timeout(std::time::Duration::from_secs(10));
        if compile {
            cmd.arg("-compile").env("TLX_PRERUN", "1");
        }
        let out = cmd.run();
        assert_eq!(out.code, 0, "{}", out.text());
        assert_eq!(std::fs::read(dir.join("inner.json")).unwrap(), b"[2]");
        assert_eq!(std::fs::read(dir.join("out.json")).unwrap(), b"[1,2]");
    }
}

/// An external command replaces a managed file with a directory. Its
/// contents cannot be restored: both the pre-run and a layout restart
/// must report the restoration failure instead of continuing the search.
#[test]
fn file_restoration_failure_stops_compiled_runs() {
    let Some(io) = tlx_testkit::require_third_party("file_restoration_failure_stops_compiled_runs", "community/IOUtils.tla") else {
        return;
    };
    let spec = r#"---- MODULE RestoreFailure ----
EXTENDS Naturals, Sequences, TLC, Json, IOUtils
VARIABLE q
Init == q = <<>>
Next == /\ Len(q) < 3
        /\ JsonSerialize("out.json", <<1>>)
        /\ IOExec(<<"sh", "-c", "rm out.json && mkdir out.json">>).exitValue = 0
        /\ q' = Append(q, 0)
====
"#;
    for (prerun, retain) in [("1", "1024"), ("1", "0"), ("2", "1024")] {
        let dir = spec_dir("RestoreFailure", spec, "INIT Init\nNEXT Next\nCHECK_DEADLOCK FALSE\n");
        std::fs::write(dir.join("out.json"), "[888]").unwrap();
        let mut cmd = tlx_testkit::tlx!();
        cmd.check("RestoreFailure.tla").arg("-compile").includes([io.parent().unwrap()]).dir(&dir);
        cmd.envs([("TLX_PRERUN", prerun), ("TLX_RETAIN_MB", retain), ("TLX_PRERUN_MS", "0")]);
        let out = cmd.run();
        let text = out.text();
        assert_ne!(out.code, 0, "{text}");
        assert!(text.contains("cannot restore file out.json"), "{text}");
        assert!(!text.contains("panicked"), "{text}");
        assert!(dir.join("out.json").is_dir());
    }
}

/// Actions that print run natively (the printing regions in the host):
/// their output is held back until the native run succeeds, so a run that
/// fails after printing (here: a successor outside a layout that missed
/// it) and is redone by the interpreter prints once, and a printing region
/// is never memoized (`<<"B", x>>` has the same arguments in several
/// states). Every line as often as the interpreter prints it.
#[test]
fn native_actions_print_like_the_interpreter() {
    let spec = r#"---- MODULE NatPrint ----
EXTENDS Naturals, TLC
VARIABLES x, y
Init == x = 0 /\ y = 0
A == x < 8 /\ PrintT(<<"A", x, y>>) /\ x' = x + 1 /\ y' = y
B == /\ y < 3
     /\ \/ x > 4 /\ PrintT(<<"B", x>>) /\ y' = y + 1
        \/ x <= 4 /\ y' = y
     /\ x' = x
Next == A \/ B
====
"#;
    let dir = spec_dir("NatPrint", spec, "INIT Init\nNEXT Next\nCHECK_DEADLOCK FALSE\n");
    let (_, interp) = run(&dir, "NatPrint", &[], &[]);
    assert_eq!(interp.iter().filter(|l| l.starts_with("<<\"B\"")).count(), 12, "interpreter output:\n{interp:?}");
    let (text, compiled) = run(&dir, "NatPrint", &[("TLX_NATIVE_SYNC", "1")], &["-compile"]);
    assert!(text.contains("native code: 2/2 actions"), "not native:\n{text}");
    assert_eq!(compiled, interp, "-compile:\n{text}");
    for jit in ["0", "1"] {
        let (text, resumed) = run(&dir, "NatPrint", &[("TLX_PRERUN", "1"), ("TLX_JIT", jit)], &["-compile"]);
        assert!(text.contains("layout miss"), "no layout miss:\n{text}");
        assert_eq!(resumed, interp, "-compile, TLX_JIT={jit}, misses:\n{text}");
    }
    let (text, w4) = run(&dir, "NatPrint", &[("TLX_PRERUN", "1")], &["-compile", "-workers", "4"]);
    assert_eq!(sorted(w4), sorted(interp), "-compile -workers 4:\n{text}");
}

/// A startup-cached recursive function keeps its value, but evaluations
/// whose file writes were rolled back must be performed by the search again.
#[test]
fn constant_lazy_function_writes_survive_rollback() {
    let spec = r#"---- MODULE ConstantMemo ----
EXTENDS Naturals, Sequences, TLC, Json
VARIABLE q
F[i \in 1..2] == IF i = 1 THEN IF JsonSerialize("inner.json", <<0>>) THEN 1 ELSE 0 ELSE F[i-1] + 1
Init == q = <<>>
Next == /\ Len(q) < 3
        /\ JsonSerialize("out.json", F)
        /\ q' = Append(q, 0)
====
"#;
    for ndjson in [false, true] {
        let spec = if ndjson { spec.replace("JsonSerialize", "ndJsonSerialize") } else { spec.to_owned() };
        let dir = spec_dir("ConstantMemo", &spec, "INIT Init\nNEXT Next\nCHECK_DEADLOCK FALSE\n");
        let (interp, _) = run(&dir, "ConstantMemo", &[], &[]);
        let files = ["inner.json", "out.json"];
        let expected = files.map(|f| std::fs::read(dir.join(f)).unwrap());
        for (prerun, retain, millis) in [("8", "1024", "1000"), ("1", "1024", "0"), ("1", "0", "0")] {
            for f in files {
                std::fs::remove_file(dir.join(f)).unwrap();
            }
            let (compiled, _) = run(
                &dir,
                "ConstantMemo",
                &[("TLX_PRERUN", prerun), ("TLX_PRERUN_REUSE", "0"), ("TLX_RETAIN_MB", retain), ("TLX_PRERUN_MS", millis)],
                &["-compile"],
            );
            if prerun == "8" {
                assert!(
                    compiled.contains("4 states") && !compiled.contains("layout miss"),
                    "pre-run did not cover the search:\n{compiled}"
                );
            } else {
                assert!(compiled.contains("layout miss"), "no layout restart:\n{compiled}");
                assert_eq!(compiled.contains("resuming the search"), retain != "0", "{compiled}");
            }
            assert_eq!(tlx_testkit::cli::result_lines(&compiled), tlx_testkit::cli::result_lines(&interp));
            for (f, bytes) in files.iter().zip(&expected) {
                assert_eq!(std::fs::read(dir.join(f)).unwrap_or_default(), *bytes, "{f}, prerun={prerun}, retain={retain}:\n{compiled}");
            }
        }
    }
}
