//! Every execution tier (interpreter, `-compile` with and without
//! native code) reports the same result as the interpreter, also where the
//! compiled tiers fall back to it: evaluation errors in invariants and
//! action properties keep their context, an Init the IR cannot run is
//! computed by the interpreter, and side effects of a failed IR run are not
//! repeated by the fallback.

use tlx_testkit::cli::{result_lines, tests_specs, TIERS};

/// Check spec `name` in each tier: (tier, exit code, output).
fn run_tiers(name: &str) -> Vec<(&'static str, i32, String)> {
    TIERS
        .iter()
        .map(|(tier, args, env)| {
            let out = tlx_testkit::tlx!()
                .check(format!("{name}.tla"))
                .cfg(format!("{name}.cfg"))
                .args(["-workers", "1"])
                .args(*args)
                .dir(tests_specs())
                .envs(env.iter().copied())
                .run();
            (*tier, out.code, out.text())
        })
        .collect()
}

/// Every tier reports what the interpreter does; its exit code and output.
fn check_tiers_agree(name: &str) -> (i32, String) {
    let runs = run_tiers(name);
    let (_, code, out) = &runs[0];
    for (tier, c, o) in &runs[1..] {
        assert_eq!(
            (c, result_lines(o)),
            (code, result_lines(out)),
            "{name}: {tier} differs from the interpreter:\n{o}\n--- interpreter:\n{out}"
        );
    }
    (*code, out.clone())
}

#[test]
fn invariant_evaluation_error() {
    let (code, out) = check_tiers_agree("EK5");
    assert_eq!(code, 76, "{out}");
    assert!(out.contains("Error: Evaluating invariant Inv failed.\nAttempted to access index 3 of tuple"), "{out}");
    assert!(out.contains("The average outdegree of the complete state graph is 1 "), "{out}");
}

#[test]
fn action_property_evaluation_error() {
    let (code, out) = check_tiers_agree("EK6");
    assert_eq!(code, 75, "{out}");
    assert!(out.contains("Error: Evaluating action property AP failed.\nAttempted to access index 3 of tuple"), "{out}");
    // The trace ends with the successor of the step.
    assert!(out.contains("State 3: <Next line 8, col 9 to line 8, col 24 of module EK6>\nx = 2\n"), "{out}");
}

#[test]
fn init_falls_back_to_the_interpreter() {
    let (code, out) = check_tiers_agree("IR1");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("8 states generated, 8 distinct states found, 0 states left on queue."), "{out}");
}

#[test]
fn side_effects_of_a_failed_ir_action_happen_once() {
    let (code, out) = check_tiers_agree("IR2");
    assert_eq!(code, 0, "{out}");
}

#[test]
fn side_effects_of_a_failed_ir_init_happen_once() {
    let (code, out) = check_tiers_agree("IR3");
    assert_eq!(code, 0, "{out}");
}

/// The `compile:` lines of output `out`.
fn compile_lines(out: &str) -> Vec<&str> {
    out.lines().filter(|l| l.starts_with("compile:")).collect()
}

/// Check IR1 with `args` and `env` and the default tier (no tier flag
/// added): (exit code, output).
fn run_default(args: &[&str], env: &[(&str, &str)]) -> (i32, String) {
    let out = tlx_testkit::tlx_default!().check("IR1.tla").cfg("IR1.cfg").args(args).dir(tests_specs()).envs(env.iter().copied()).run();
    (out.code, out.text())
}

#[test]
fn the_default_tier_is_the_compiled_tier() {
    let (code, interp) = run_default(&["-interp"], &[]);
    assert_eq!(code, 0, "{interp}");
    assert!(compile_lines(&interp).is_empty(), "-interp must not compile:\n{interp}");
    for args in [&[][..], &["-compile"], &["-interp", "-compile"]] {
        let (code, out) = run_default(args, &[]);
        assert_eq!(code, 0, "{args:?}: {out}");
        assert!(!compile_lines(&out).is_empty(), "{args:?} must run the compiled tier:\n{out}");
        assert_eq!(result_lines(&out), result_lines(&interp), "{args:?}");
    }
    // (The last tier flag wins.)
    let (_, out) = run_default(&["-compile", "-interp"], &[]);
    assert!(compile_lines(&out).is_empty(), "{out}");
}

/// Without a C compiler the default tier keeps the JIT code, or, with no
/// JIT either, checks with the interpreter: one line says so.
#[test]
fn the_default_tier_without_a_c_compiler() {
    let cache = std::env::temp_dir().join(format!("tlx-tiers-nocc-{}", std::process::id()));
    let cache = cache.to_str().unwrap();
    let (_, interp) = run_default(&["-interp"], &[]);
    let no_cc = [("CC", "/nonexistent/cc"), ("TLX_CACHE", cache), ("TLX_NATIVE_SYNC", "1")];
    let (code, out) = run_default(&[], &no_cc);
    assert_eq!(code, 0, "{out}");
    assert_eq!(result_lines(&out), result_lines(&interp));
    let failed: Vec<_> = compile_lines(&out).into_iter().filter(|l| l.contains("C compiler")).collect();
    assert_eq!(failed, ["compile: C build failed, using the JIT code: no C compiler (/nonexistent/cc not found)"], "{out}");
    let (code, out) = run_default(&[], &[("TLX_JIT", "0"), no_cc[0], no_cc[1]]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(result_lines(&out), result_lines(&interp));
    assert_eq!(
        compile_lines(&out),
        ["compile: no JIT and no C compiler (/nonexistent/cc not found): checking with the interpreter"],
        "{out}"
    );
    // (Nothing was written into the cache.)
    assert!(std::fs::read_dir(cache).map_or(true, |mut d| d.next().is_none()), "files in {cache}");
    let _ = std::fs::remove_dir_all(cache);
}
