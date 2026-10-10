//! Value-contract litmus specs (docs/value-store.md §9):
//! function aliases, the empty function, EXCEPT on missing paths, CHOOSE
//! order over records with set-valued fields and sets of sets, and lazy
//! union duplicates. Each runs on the interpreter and on the three compiled
//! executors (closure IR, C, JIT) under `TLX_PARANOID`, which compares every
//! compiled result with the interpreter's; all must report TLC's counts
//! (cached in tests/specs/tlc/, checked against TLC by scripts/spectest.py).

use tlx_testkit::cli::{counts, tests_specs};

/// (name, extra `-compile` environment, column of the actions table that
/// must show calls: the executor under test ran).
const BACKENDS: [(&str, &[(&str, &str)], &str); 3] =
    [("closure IR", &[("TLX_NO_NATIVE", "1")], "ir"), ("C", &[("TLX_JIT", "0")], "native"), ("JIT", &[("TLX_JIT", "1")], "native")];

/// The litmus specs and TLC's counts for them.
const SPECS: [(&str, &str, &str); 6] = [
    (
        "ReprAlias",
        "10 states generated, 8 distinct states found, 0 states left on queue.",
        "The depth of the complete state graph search is 5.",
    ),
    (
        "ReprEmptyFcn",
        "11 states generated, 10 distinct states found, 0 states left on queue.",
        "The depth of the complete state graph search is 5.",
    ),
    (
        "ReprMissingPath",
        "10 states generated, 10 distinct states found, 0 states left on queue.",
        "The depth of the complete state graph search is 3.",
    ),
    (
        "ReprChooseOrder",
        "5 states generated, 5 distinct states found, 0 states left on queue.",
        "The depth of the complete state graph search is 5.",
    ),
    (
        "ReprLazyUnion",
        "13 states generated, 3 distinct states found, 0 states left on queue.",
        "The depth of the complete state graph search is 2.",
    ),
    (
        "ReprExceptEffects",
        "7 states generated, 7 distinct states found, 0 states left on queue.",
        "The depth of the complete state graph search is 3.",
    ),
];

/// The value store modes (`TLX_VALUE_STORE`): the slots that would be
/// `Dyn` (ReprExceptEffects' bag), every slot.
const STORES: [&str; 2] = ["1", "all"];

/// The calls in column `col` of the `-stats` actions table, summed over
/// the actions (also the mixed column for `native`: mixed calls run native
/// code with host regions).
fn calls(out: &str, col: &str) -> u64 {
    let mut lines = out.lines().skip_while(|l| !l.starts_with("actions "));
    let Some(head) = lines.next() else { return 0 };
    let cols: Vec<&str> = head.split_whitespace().skip(1).collect();
    let want: Vec<usize> =
        cols.iter().enumerate().filter(|(_, c)| **c == col || (col == "native" && **c == "mixed")).map(|(i, _)| i).collect();
    let mut total = 0;
    for l in lines.take_while(|l| l.starts_with("  ")) {
        // The name may contain spaces: the numbers are the last columns.
        let words: Vec<&str> = l.split_whitespace().collect();
        let nums = &words[words.len() - (cols.len() - 1)..];
        for &i in &want {
            total += nums.get(i).map_or(0, |n| n.replace(',', "").parse::<u64>().unwrap_or(0));
        }
    }
    total
}

/// TLC prints `<<"present", 0>>` once for ReprExceptEffects (the present
/// key's replacement), and nothing for the absent keys.
fn check_effects(name: &str, what: &str, text: &str) {
    if name == "ReprExceptEffects" {
        assert_eq!(text.matches("<<\"present\", 0>>").count(), 1, "{what}\n{text}");
        assert!(!text.contains("\"absent\""), "{what}\n{text}");
    }
}

fn run(name: &str, compile: bool, env: &[(&str, &str)]) -> tlx_testkit::cli::Run {
    let mut cmd = tlx_testkit::tlx!();
    cmd.check(format!("{name}.tla")).cfg(format!("{name}.cfg")).args(["-workers", "1", "-stats"]).dir(tests_specs());
    if compile {
        cmd.arg("-compile").env("TLX_PARANOID", "1").envs(env.iter().copied());
    }
    cmd.run()
}

#[test]
fn litmus_specs_agree_with_tlc_on_every_executor() {
    for (name, generated, depth) in SPECS {
        let interp = run(name, false, &[]);
        assert_eq!(counts(&interp.text()), [generated, depth], "{name}: interpreter\n{}", interp.text());
        check_effects(name, "interpreter", &interp.text());
        for (backend, env, col) in BACKENDS {
            let out = run(name, true, env);
            let text = out.text();
            assert_eq!(out.code, 0, "{name}: {backend}\n{text}");
            assert_eq!(counts(&text), [generated, depth], "{name}: {backend}\n{text}");
            assert!(calls(&text, col) > 0, "{name}: {backend} did not run ({col} calls)\n{text}");
            assert_eq!(text.contains("native code (JIT) ready"), backend == "JIT", "{name}: {backend}\n{text}");
            check_effects(name, backend, &text);
        }
    }
}

/// The same with value store slots (`TLX_VALUE_STORE`, design batch 1):
/// the operations on handles in every executor, against TLC's counts and
/// output, with the store's statistics showing that it was used.
#[test]
fn litmus_specs_agree_with_tlc_with_value_store_slots() {
    for store in STORES {
        for (name, generated, depth) in SPECS {
            for (backend, env, _) in BACKENDS {
                let mut env = env.to_vec();
                env.push(("TLX_VALUE_STORE", store));
                let out = run(name, true, &env);
                let (what, text) = (format!("{name}: {backend}, TLX_VALUE_STORE={store}"), out.text());
                assert_eq!(out.code, 0, "{what}\n{text}");
                assert_eq!(counts(&text), [generated, depth], "{what}\n{text}");
                check_effects(name, &what, &text);
                if store == "all" || name == "ReprExceptEffects" {
                    assert!(text.contains("value store nodes"), "{what}: no value store\n{text}");
                }
            }
        }
        // A replacement that fails on a present key is the run's error.
        for (backend, env, _) in BACKENDS {
            let mut env = env.to_vec();
            env.push(("TLX_VALUE_STORE", store));
            let out = run("ReprExceptError", true, &env);
            let text = out.text();
            assert_ne!(out.code, 0, "{backend}, TLX_VALUE_STORE={store}\n{text}");
            assert!(text.contains("2 states generated, 2 distinct states found"), "{backend}, TLX_VALUE_STORE={store}\n{text}");
        }
    }
}

/// compat.md D-13: the fixed encodings do not record how a function was
/// built. `[i \in {1, 2} |-> i]` is equal to the tuple `<<1, 2>>`, but
/// EXCEPT with a model-value key leaves the function unchanged (TLC warns),
/// where the tuple is an error. TLC and the interpreter: 6 generated / 5
/// distinct / depth 4; the compiled tier decodes the tuple and reports the
/// error.
///
/// Which slots still have D-13: every slot with a fixed encoding, that is
/// every slot without `TLX_VALUE_STORE`, and with `TLX_VALUE_STORE=1` every
/// slot whose shape has no `Dyn` part (`x` here). Value store (`Ref`) slots
/// keep the construction and do not (`TLX_VALUE_STORE=all` makes every
/// slot one); the final cutover to stable roots must close D-13 for all
/// slots (docs/value-store.md §8).
#[test]
fn function_witness_of_fixed_encodings_is_the_documented_difference() {
    const SPEC: &str = r#"---- MODULE Witness ----
EXTENDS Naturals
CONSTANT m
VARIABLES x, pc
Init == x = <<1, 2>> /\ pc = 0
Next == \/ pc = 0 /\ x' = [i \in {1, 2} |-> i] /\ pc' = 1
        \/ pc = 0 /\ x' = x /\ pc' = 5
        \/ pc = 5 /\ x' = <<1, 2>> /\ pc' = 1
        \/ pc = 1 /\ x' = [x EXCEPT ![m] = 0] /\ pc' = 2
        \/ pc = 2 /\ x' = [x EXCEPT ![1] = 3] /\ pc' = 3
Inv == x \in {<<1, 2>>, <<3, 2>>}
====
"#;
    const CFG: &str = "CONSTANT m = m\nINIT Init\nNEXT Next\nINVARIANT Inv\nCHECK_DEADLOCK FALSE\n";
    let dir = tlx_testkit::TempDir::with_spec("repr-witness", SPEC, CFG);
    let check = |compile: bool, env: &[(&str, &str)]| {
        let mut cmd = tlx_testkit::tlx!();
        cmd.check("Witness.tla").cfg("Witness.cfg").args(["-workers", "1"]).dir(dir.path());
        if compile {
            cmd.arg("-compile").envs(env.iter().copied());
        }
        cmd.run()
    };
    let interp = check(false, &[]);
    assert_eq!(
        counts(&interp.text()),
        ["6 states generated, 5 distinct states found, 0 states left on queue.", "The depth of the complete state graph search is 4."],
        "{}",
        interp.text()
    );
    for (backend, env, _) in BACKENDS {
        for store in ["0", "1"] {
            let mut env = env.to_vec();
            env.push(("TLX_VALUE_STORE", store));
            let out = check(true, &env);
            let text = out.text();
            assert!(
                text.contains("Attempted to access tuple at a non integral index: m"),
                "{backend}, store {store}: D-13 changed?\n{text}"
            );
        }
    }
    // Value store slots keep how each function was built: no D-13 there
    // (here every slot is one, `TLX_VALUE_STORE=all`).
    for (backend, env, _) in BACKENDS {
        let mut env = env.to_vec();
        env.push(("TLX_VALUE_STORE", "all"));
        let out = check(true, &env);
        assert_eq!(
            counts(&out.text()),
            ["6 states generated, 5 distinct states found, 0 states left on queue.", "The depth of the complete state graph search is 4."],
            "{backend}, TLX_VALUE_STORE=all\n{}",
            out.text()
        );
    }
}
