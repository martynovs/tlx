//! The CommunityModules are bundled with tlx, as TLC finds them in its jars:
//! a spec extends them without `-I`, and modules in the spec's directory or
//! in `-I` directories take precedence over the bundled ones (standard
//! modules included), in TLC's lookup order (`SimpleFilenameToStream`).

use std::path::Path;

fn temp_dir(tag: &str) -> tlx_testkit::TempDir {
    tlx_testkit::TempDir::new(&format!("bundled_modules-{tag}"))
}

/// Run `tlx check Spec.tla` in `dir` with `args`; (exit code, output).
fn check(dir: &Path, args: &[&str]) -> (i32, String) {
    let mut cmd = tlx_testkit::tlx!();
    cmd.args(["check", "Spec.tla", "-config", "Spec.cfg", "-workers", "1"]).args(args).dir(dir);
    let out = cmd.run();
    (out.code, out.text())
}

fn write_spec(dir: &Path, body: &str) {
    std::fs::write(dir.join("Spec.tla"), format!("---- MODULE Spec ----\n{body}\nVARIABLE x\nInit == x = 0\nNext == x' = x\n====\n"))
        .unwrap();
    std::fs::write(dir.join("Spec.cfg"), "INIT Init\nNEXT Next\n").unwrap();
}

#[test]
fn community_modules_need_no_search_path() {
    let dir = temp_dir("plain");
    write_spec(
        &dir,
        "EXTENDS SequencesExt, FiniteSetsExt, Functions, Folds, Relation, Bitwise, TLC, Integers\n\
         LOCAL G == INSTANCE Graphs\n\
         ASSUME Reverse(<<1, 2, 3>>) = <<3, 2, 1>>\n\
         ASSUME SumSet(1..10) = 55\n\
         ASSUME Range([i \\in 1..3 |-> i * i]) = {1, 4, 9}\n\
         ASSUME IsReflexive([p \\in {1} \\X {1} |-> TRUE], {1})\n\
         ASSUME 5 & 3 = 1\n\
         ASSUME G!SimplePath([node |-> {1, 2}, edge |-> {<<1, 2>>}]) = {<<1>>, <<2>>, <<1, 2>>}",
    );
    let (code, out) = check(&dir, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("No error has been found"), "{out}");
}

#[test]
fn search_paths_take_precedence() {
    let dir = temp_dir("override");
    let lib = dir.join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    // A community module and a standard module of the same names: both are
    // read from the -I directory (TLC's -DTLA-Library precedes its jar);
    // the Java-overridden operators stay native (Cardinality).
    std::fs::write(lib.join("Folds.tla"), "---- MODULE Folds ----\nMarkerFolds == 7\n====\n").unwrap();
    std::fs::write(
        lib.join("FiniteSets.tla"),
        "---- MODULE FiniteSets ----\nLOCAL INSTANCE Naturals\nIsFiniteSet(S) == TRUE\nCardinality(S) == 42\nMarkerFS == 1\n====\n",
    )
    .unwrap();
    write_spec(&dir, "EXTENDS Folds, FiniteSets, TLC\nASSUME MarkerFolds = 7 /\\ MarkerFS = 1 /\\ Cardinality({1, 2}) = 2");
    let (code, out) = check(&dir, &["-I", "lib"]);
    assert_eq!(code, 0, "{out}");
    // Without -I the bundled modules have no markers.
    let (code, out) = check(&dir, &[]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("MarkerFolds"), "{out}");
    // The spec's directory comes first.
    std::fs::rename(lib.join("Folds.tla"), dir.join("Folds.tla")).unwrap();
    std::fs::write(lib.join("Folds.tla"), "---- MODULE Folds ----\nMarkerFolds == 8\n====\n").unwrap();
    let (code, out) = check(&dir, &["-I", "lib"]);
    assert_eq!(code, 0, "{out}");
}
