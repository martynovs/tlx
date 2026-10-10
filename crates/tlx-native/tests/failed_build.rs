//! A failed C build must remove its temporary source, output and log files,
//! and still report the compiler error.

use tlx_native::build::{build, BuildOptions};

/// A translation unit the C compiler always rejects.
const BROKEN: &str = "#error forced failure\n";

#[test]
fn failed_build_removes_temporaries() {
    let cache = std::env::temp_dir().join(format!("tlx-native-failed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let opts = BuildOptions { cache_dir: cache.clone(), ..Default::default() };
    let err = build(BROKEN, &opts).expect_err("the build must fail");
    assert!(err.contains("C compilation failed") && err.contains("forced failure"), "unexpected error: {err}");
    // Only the ABI header and the build lock of the cache slot may remain:
    // the temporary source, the (possibly partial) output and the log of
    // this build must be gone.
    let mut names = Vec::new();
    for slot in std::fs::read_dir(&cache).expect("cache dir") {
        for f in std::fs::read_dir(slot.expect("slot").path()).expect("slot dir") {
            names.push(f.expect("entry").file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    assert_eq!(names, ["build.lock", "tlx_abi.h"], "temporary files left in the cache: {names:?}");
    let _ = std::fs::remove_dir_all(&cache);
}

/// A compiler that fails after writing a partial library, or exits zero
/// without producing one, must never publish a cache hit.
#[test]
fn incomplete_compiler_output_is_rejected() {
    use std::os::unix::fs::PermissionsExt;

    for (name, command) in [("partial", "printf partial > \"$TLX_TMP\"; echo forced failure >&2; exit 7"), ("missing", "exit 0")] {
        let dir = std::env::temp_dir().join(format!("tlx-native-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("cc.sh");
        std::fs::write(&script, format!("#!/bin/sh\ntrap 'printf \"%s\\n\" \"$?\" > \"$0.exit\"' 0\nprintf '%s\\n' \"$0\" \"$@\" > \"$0.argv\"\npwd > \"$0.cwd\"\nprintf '%s\\n' \"$TLX_TMP\" \"$TLX_LIB\" \"$TLX_SRC\" \"$TLX_LOG\" \"$TLX_KEEP\" > \"$0.env\"\n{command}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let opts = BuildOptions { cc: script.display().to_string(), opt: "-O0".into(), debug: false, cache_dir: dir.join("cache") };
        let error = build("int model(void) { return 0; }", &opts).expect_err("incomplete output must fail");
        assert!(error.contains("C compilation failed"), "{error}");
        if name == "partial" {
            assert!(error.contains("forced failure"), "{error}");
        }
        assert_eq!(std::fs::read_to_string(script.with_extension("sh.exit")).unwrap().trim(), if name == "partial" { "7" } else { "0" });
        let env = std::fs::read_to_string(script.with_extension("sh.env")).unwrap();
        let paths: Vec<_> = env.lines().collect();
        assert_eq!(paths.len(), 5);
        let slot = std::path::Path::new(paths[0]).parent().unwrap();
        let argv = std::fs::read_to_string(script.with_extension("sh.argv")).unwrap();
        let expected = [
            opts.cc.as_str(),
            "-O0",
            "-std=c11",
            "-fPIC",
            "-shared",
            "-fno-strict-aliasing",
            "-Wno-unused",
            "-I",
            slot.to_str().unwrap(),
            "-o",
            paths[0],
            paths[2],
        ];
        assert_eq!(argv.lines().collect::<Vec<_>>(), expected);
        assert_eq!(
            std::fs::read_to_string(script.with_extension("sh.cwd")).unwrap().trim(),
            std::env::current_dir().unwrap().to_str().unwrap()
        );
        assert_eq!(std::path::Path::new(paths[1]).parent(), Some(slot));
        assert_eq!(std::path::Path::new(paths[3]).parent(), Some(slot));
        assert_eq!(std::path::Path::new(paths[4]), slot.join("model.c"));
        for path in paths {
            assert!(!std::path::Path::new(path).exists(), "incomplete build left {path}");
        }
        assert!(tlx_native::build::cached("int model(void) { return 0; }", &opts).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
