//! Parse every `.tla` file of the bundled standard and Community modules and
//! of the tlaplus/Examples corpus (`third_party/Examples`, populated by
//! `scripts/fetch-deps.sh`) and require lowering to succeed.

use std::path::{Path, PathBuf};

use tlx_front::ast::FileId;
use tlx_front::parse::parse_module;

/// Files that are expected to fail, relative to the repo root, with a substring
/// that every reported diagnostic must contain. Currently empty: every corpus
/// file lowers (subexpression selectors `Inv!2`, `Thm!:` are supported).
const ALLOWLIST: &[(&str, &str)] = &[];

/// Bundled module directories owned by `tlx-sema` (ARCH-09). Each one is
/// required to exist and contribute at least one `.tla` file; a missing or
/// empty directory means the test silently stopped covering the bundled
/// modules, so it must fail loudly instead.
const BUNDLED_MODULE_DIRS: &[&str] = &["crates/tlx-sema/modules/stdlib", "crates/tlx-sema/modules/community"];

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let rd = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("module directory {}: {e}", dir.display()));
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "tla") && !p.to_string_lossy().contains("_TTrace_") {
            // (`*_TTrace_*.tla` are trace files left by TLC runs.)
            out.push(p);
        }
    }
}

#[test]
fn corpus_parses() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let mut files = Vec::new();
    for dir in BUNDLED_MODULE_DIRS {
        let before = files.len();
        collect(&root.join(dir), &mut files);
        assert!(files.len() > before, "{dir}: configured bundled module directory has no .tla files");
    }
    // (Without the corpus only the bundled modules are checked.)
    if let Some(examples) = tlx_testkit::require_third_party("corpus_parses", "Examples/specifications") {
        collect(&examples, &mut files);
    }
    files.sort();
    let mut failures = Vec::new();
    let mut unexpected_ok = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap_or(f).to_string_lossy().replace('\\', "/");
        let src = match std::fs::read(f) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => panic!("{rel}: {e}"),
        };
        let allowed = ALLOWLIST.iter().find(|(p, _)| *p == rel).map(|(_, why)| *why);
        match parse_module(&src, FileId(0)) {
            Ok(_) if allowed.is_some() => unexpected_ok.push(rel),
            Ok(_) => {}
            Err(diags) if allowed.is_some_and(|why| diags.iter().all(|d| d.message.contains(why))) => {}
            Err(diags) => {
                let first: Vec<String> = diags.iter().take(3).map(|d| d.to_string()).collect();
                failures.push(format!("{rel} ({} diags): {}", diags.len(), first.join(" | ")));
            }
        }
    }
    println!("parsed {} files ({} allowlisted)", files.len(), ALLOWLIST.len());
    for u in &unexpected_ok {
        println!("note: allowlisted file now parses: {u}");
    }
    if !failures.is_empty() {
        panic!("{} of {} files failed:\n{}", failures.len(), files.len(), failures.join("\n"));
    }
}
