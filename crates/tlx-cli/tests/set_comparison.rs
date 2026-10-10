//! Cached TLC regressions for fallible comparisons in set fast paths.

use tlx_testkit::cli::tests_specs;

#[test]
fn set_comparison_errors_and_effects_on_every_executor() {
    for store in ["0", "1", "all"] {
        for (backend, env) in [
            ("interpreter", vec![]),
            ("closure", vec![("TLX_NO_NATIVE", "1")]),
            ("C", vec![("TLX_JIT", "0")]),
            ("JIT", vec![("TLX_JIT", "1"), ("TLX_JIT_VERIFY", "1")]),
        ] {
            for (spec, error, generated, distinct) in [
                ("UnionComparisonError", true, 1, 1),
                ("UnionAssignmentComparisonError", true, 2, 2),
                ("UnionSubsetComparison", false, 2, 1),
                ("UnionTailComparison", true, 1, 1),
                ("SetDifferenceComparisonEffects", false, 2, 1),
            ] {
                let mut cmd = tlx_testkit::tlx!();
                cmd.check(format!("{spec}.tla"))
                    .cfg(format!("{spec}.cfg"))
                    .dir(tests_specs())
                    .args(["-workers", "1"])
                    .env("TLX_VALUE_STORE", store)
                    .env("TLX_NATIVE_SYNC", "1")
                    .env("TLX_IR_VERIFY_HARD", "1")
                    .envs(env.iter().copied());
                if backend != "interpreter" {
                    cmd.arg("-compile").env("TLX_PARANOID", "1");
                }
                let out = cmd.run();
                let text = out.text();
                let context = format!("{spec}, {backend}, store {store}\n{text}");
                assert_ne!(out.code, 101, "{context}");
                assert_eq!(out.code != 0, error, "{context}");
                assert!(text.contains(&format!("{generated} states generated, {distinct} distinct states found,")), "{context}");
                assert!(text.contains("The depth of the complete state graph search is 1."), "{context}");
            }
        }
    }
}
