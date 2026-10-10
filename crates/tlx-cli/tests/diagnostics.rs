//! Parse and semantic errors name the file they are in, also when the error
//! is in a module the spec EXTENDS (FE-7). The path is the one the user would
//! recognize: relative to the working directory when the module is under it.

use std::path::Path;

const A: &str = "---- MODULE A ----\nEXTENDS B\nVARIABLE x\nInit == x = 0\nNext == x' = x\n====\n";
const CFG: &str = "INIT Init\nNEXT Next\n";

/// A fresh directory with A.tla, A.cfg and the given B.tla.
fn setup(b: &str) -> tlx_testkit::TempDir {
    let dir = tlx_testkit::TempDir::new("diagnostics");
    std::fs::write(dir.join("A.tla"), A).unwrap();
    std::fs::write(dir.join("A.cfg"), CFG).unwrap();
    std::fs::write(dir.join("B.tla"), b).unwrap();
    dir
}

/// Run `tlx check <spec>` in `dir`; (exit code, stdout, stderr). The
/// diagnostics go to stderr, TLC's `Error: ...` summary to stdout.
fn check(dir: &Path, spec: &str) -> (i32, String, String) {
    let out = tlx_testkit::tlx!().args(["check", spec]).dir(dir).run();
    (out.code, out.stdout, out.stderr)
}

#[test]
fn semantic_error_in_extended_module_names_its_file() {
    let dir = setup("---- MODULE B ----\nFoo == Undefined + 1\n====\n");
    let (code, out, err) = check(&dir, "A.tla");
    assert_eq!(code, 150, "{out}{err}");
    assert!(err.contains("B.tla:2:8: Unknown operator: `Undefined'.\n"), "{err}");
    assert!(err.contains("B.tla:2:18: Unknown operator: `+'.\n"), "{err}");
    assert!(out.ends_with("Error: Parsing or semantic analysis failed.\n"), "{out}");
    // No bare `line:col` diagnostics left.
    assert!(!err.lines().any(|l| l.starts_with("2:")), "{err}");
}

#[test]
fn syntax_error_in_extended_module_names_its_file() {
    let dir = setup("---- MODULE B ----\nFoo == (1\n====\n");
    let (code, out, err) = check(&dir, "A.tla");
    assert_eq!(code, 150, "{out}{err}");
    assert!(err.lines().any(|l| l.starts_with("B.tla:2:1: syntax error at or after")), "{err}");
}

#[test]
fn error_in_root_module_names_its_file() {
    let dir = setup("---- MODULE B ----\nFoo == 1\n====\n");
    std::fs::write(dir.join("A.tla"), "---- MODULE A ----\nEXTENDS B\nBar == Undefined\n====\n").unwrap();
    let (code, out, err) = check(&dir, "A.tla");
    assert_eq!(code, 150, "{out}{err}");
    assert!(err.contains("A.tla:3:8: Unknown operator: `Undefined'.\n"), "{err}");
}

#[test]
fn absolute_spec_path_is_printed_relative_to_the_working_directory() {
    let dir = setup("---- MODULE B ----\nFoo == Undefined + 1\n====\n");
    let (code, out, err) = check(&dir, dir.join("A.tla").to_str().unwrap());
    assert_eq!(code, 150, "{out}{err}");
    assert!(err.contains("B.tla:2:8: Unknown operator: `Undefined'.\n"), "{err}");
    assert!(!err.contains(dir.to_str().unwrap()), "{err}");
}

#[test]
fn module_outside_the_working_directory_keeps_its_path() {
    let dir = setup("---- MODULE B ----\nFoo == Undefined + 1\n====\n");
    // `..` from the spec's directory: the module is not under the working
    // directory, so its path stays as it was found.
    let sub = dir.join("sub");
    std::fs::create_dir(&sub).unwrap();
    let (code, out, err) = check(&sub, "../A.tla");
    assert_eq!(code, 150, "{out}{err}");
    assert!(err.contains("../B.tla:2:8: Unknown operator: `Undefined'.\n"), "{err}");
}
