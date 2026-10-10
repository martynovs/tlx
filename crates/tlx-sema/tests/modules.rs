//! The bundled standard and Community modules are data assets of `tlx-sema`,
//! the crate that embeds them (`src/builtins.rs`). ARCH-09: the embedded bytes
//! must come from a file the crate's own package carries, never from a sibling
//! crate's private directory (a cross-crate include path turns that crate's
//! moves into compile failures here).

use std::path::{Path, PathBuf};

use tlx_sema::builtins::{COMMUNITY_MODULES, STD_MODULES};

/// `(asset subdirectory, embedded table)` pairs, in lookup order.
fn tables() -> [(&'static str, &'static [(&'static str, &'static str)]); 2] {
    [("stdlib", STD_MODULES), ("community", COMMUNITY_MODULES)]
}

fn owner_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Every embedded source is the exact content of an owner-local file, so
/// `include_str!` cannot silently resolve to a copy outside the package, and a
/// later move that leaves the package without the file fails here.
#[test]
fn embedded_sources_are_owner_local_files() {
    let root = owner_dir().join("modules");
    let mut checked = 0;
    for (dir, table) in tables() {
        for (name, src) in table {
            let path = root.join(dir).join(format!("{name}.tla"));
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(bytes, src.as_bytes(), "embedded bytes differ from {}", path.display());
            checked += 1;
        }
    }
    assert_eq!(checked, 47, "the two tables embed 47 .tla files");
    // A bundle re-added under another crate's asset directory would be a
    // cross-crate include target; the parent of this crate must have none.
    let front = owner_dir().join("../tlx-front");
    assert!(front.join("Cargo.toml").is_file(), "owner check must point at a crate");
    for dir in ["stdlib", "community"] {
        assert!(!front.join(dir).exists(), "{} must not exist", front.join(dir).display());
    }
}

/// The tables are complete and their file contents agree with their keys:
/// names are unique per table and each file's top-level module is named after
/// the file, as SANY requires.
#[test]
fn tables_are_unique_and_files_are_named_after_their_modules() {
    assert_eq!(STD_MODULES.len(), 13, "standard modules");
    assert_eq!(COMMUNITY_MODULES.len(), 34, "Community modules");
    for (dir, table) in tables() {
        let mut names: Vec<&str> = table.iter().map(|(n, _)| *n).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "duplicate module names in {dir}");
        for (name, src) in table {
            let mods = tlx_front::parse::parse_modules(src, tlx_front::ast::FileId(0))
                .unwrap_or_else(|ds| panic!("{dir}/{name}.tla: {} diagnostics", ds.len()));
            assert_eq!(&*mods[0].name.name, *name, "{dir}/{name}.tla must contain module {name}");
        }
    }
}
