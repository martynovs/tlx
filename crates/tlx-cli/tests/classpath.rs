//! `TLX_CLASSPATH` mirrors TLC's Java classpath: its directories are searched
//! after the built-in standard and community modules, so a module there does
//! not shadow a built-in one, while `-I` (TLC's `-DTLA-Library`) does.

use std::path::Path;

fn temp_dir(tag: &str) -> tlx_testkit::TempDir {
    tlx_testkit::TempDir::new(&format!("classpath-{tag}"))
}

/// Run `tlx check Spec.tla` in `dir` with `args`; (exit code, output).
fn check(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> (i32, String) {
    let mut cmd = tlx_testkit::tlx!();
    cmd.args(["check", "Spec.tla", "-config", "Spec.cfg", "-workers", "1"]).args(args).dir(dir);
    cmd.envs(env.iter().copied());
    let out = cmd.run();
    (out.code, out.text())
}

/// A `lib/` directory with the given modules, and a spec that needs CopiesIn
/// from the built-in Bags.
fn setup(tag: &str, lib: &[(&str, &str)]) -> tlx_testkit::TempDir {
    let dir = temp_dir(tag);
    let lib_dir = dir.join("lib");
    std::fs::create_dir_all(&lib_dir).unwrap();
    for (name, src) in lib {
        std::fs::write(lib_dir.join(format!("{name}.tla")), src).unwrap();
    }
    // The bag `[e \in {1, 2} |-> 3]` has 3 copies of 1.
    std::fs::write(
        dir.join("Spec.tla"),
        "---- MODULE Spec ----\nEXTENDS Bags\nVARIABLE x\nInit == x = CopiesIn(1, [e \\in {1, 2} |-> 3])\nNext == UNCHANGED x\n====\n",
    )
    .unwrap();
    std::fs::write(dir.join("Spec.cfg"), "INIT Init\nNEXT Next\n").unwrap();
    dir
}

#[test]
fn classpath_is_searched_after_the_builtin_modules() {
    // A lib/Bags.tla without CopiesIn: on the classpath it must lose to the
    // built-in Bags, with -I it must win (as TLC's -DTLA-Library).
    let dir = setup("bags", &[("Bags", "---- MODULE Bags ----\nFoo == 1\n====\n")]);
    let clspath = dir.join("lib");
    let (code, out) = check(&dir, &[], &[("TLX_CLASSPATH", clspath.to_str().unwrap())]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("No error has been found"), "{out}");
    let (code, out) = check(&dir, &["-I", "lib"], &[]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("CopiesIn"), "{out}");
}

#[test]
fn classpath_finds_modules_that_are_not_built_in() {
    let dir = temp_dir("only-here");
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/OnlyHere.tla"), "---- MODULE OnlyHere ----\nBar == 5\n====\n").unwrap();
    std::fs::write(
        dir.join("Spec.tla"),
        "---- MODULE Spec ----\nEXTENDS OnlyHere\nVARIABLE x\nInit == x = Bar\nNext == UNCHANGED x\n====\n",
    )
    .unwrap();
    std::fs::write(dir.join("Spec.cfg"), "INIT Init\nNEXT Next\n").unwrap();
    let clspath = dir.join("lib");
    let (code, out) = check(&dir, &[], &[("TLX_CLASSPATH", clspath.to_str().unwrap())]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = check(&dir, &[], &[]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("OnlyHere"), "{out}");
}
