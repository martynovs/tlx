//! Documentation modules must be parsed without requiring imports or a cfg.
#[test]
fn parse_only_does_not_resolve_imports_or_load_a_model() {
    let dir = tlx_testkit::TempDir::new("parse-only");
    std::fs::write(dir.join("Example.tla"), "---- MODULE Example ----\nEXTENDS NotInstalled\nValue == Unknown\n====\n").unwrap();
    let out = tlx_testkit::tlx!().args(["parse", "Example.tla"]).dir(&dir).run();
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.is_empty());
}

#[test]
fn parse_only_rejects_malformed_source_with_a_location() {
    let dir = tlx_testkit::TempDir::new("parse-only-bad");
    std::fs::write(dir.join("Example.tla"), "---- MODULE Example ----\nValue == (1\n====\n").unwrap();
    let out = tlx_testkit::tlx!().args(["parse", "Example.tla"]).dir(&dir).run();
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    assert!(out.stderr.contains("Example.tla:2:"), "{}", out.stderr);
}
