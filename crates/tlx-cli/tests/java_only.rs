//! Definitions TLC implements in Java, whose `.tla` body in the module is
//! only a placeholder (`TRUE`, a constant record, a bounded CHOOSE): tlx
//! must refuse to evaluate them instead of silently returning the
//! placeholder. Applying one is an evaluation error in every tier,
//! including the compiled one, which falls back to the interpreter for it
//! (docs/features.md §6.1).

use std::path::{Path, PathBuf};

/// (module, operator, an application of it to place in an invariant, code).
const CASES: &[(&str, &str, &str, i32)] = &[
    ("IOUtils", "Serialize", "Serialize(1, \"f\", <<>>)", 75),
    ("IOUtils", "Deserialize", "Deserialize(\"f\", <<>>)", 75),
    ("TLCExt", "CounterExample", "CounterExample", 75),
    ("TLCExt", "ToTrace", "ToTrace(1)", 75),
    ("TLCExt", "TLCEvalDefinition", "TLCEvalDefinition(1)", 75),
];

use tlx_testkit::cli::TIERS;

/// A fresh directory with a spec applying `app` in its invariant.
fn spec_dir(module: &str, app: &str) -> tlx_testkit::TempDir {
    let dir = tlx_testkit::TempDir::new("java_only");
    let spec = format!(
        "---- MODULE JO ----\nEXTENDS {module}, TLC, Sequences\nVARIABLE x\n\
         Init == x = 0\nNext == x' = x\nInv == {app} = 0\n====\n"
    );
    std::fs::write(dir.join("JO.tla"), spec).unwrap();
    std::fs::write(dir.join("JO.cfg"), "INIT Init\nNEXT Next\nINVARIANT Inv\n").unwrap();
    dir
}

/// Run `tlx check` on the spec in `dir` with `args`, `env` and module
/// search `library` paths (`-I`).
fn run(dir: &Path, args: &[&str], env: &[(&str, &str)], library: &[PathBuf]) -> (i32, String) {
    let mut cmd = tlx_testkit::tlx!();
    cmd.args(["check", "JO.tla", "-config", "JO.cfg", "-workers", "1"]).args(args).dir(dir);
    cmd.includes(library);
    let out = cmd.envs(env.iter().copied()).run();
    (out.code, out.text())
}

/// `TLCExt!AssertError(err, exp)` compares `err` with the message of the
/// error `exp` raises (TLCExt.java `assertError`), as the CommunityModules
/// tests use it. TLC 1813307 passes every ASSUME of this module when run
/// without `-tool`; with `-tool` its messages carry the tool markers, so
/// no AssertError of it holds and the spec tests (which run TLC with
/// `-tool`) cannot check it.
const ASSERT_ERROR_SPEC: &str = r#"---- MODULE JO ----
EXTENDS Integers, Sequences, FiniteSets, TLC, TLCExt, FiniteSetsExt, Functions, SequencesExt, BagsExt, Folds, IOUtils
NoneOf(S) == CHOOSE x \in S : FALSE
ASSUME AssertError("The third argument of FoldFunction should be a function, but instead it is:\nTRUE",
                   FoldFunction(+, 23, TRUE))
ASSUME AssertError("The third argument of FoldFunction should be a function, but instead it is:\nTRUE",
                   FoldSeq(+, 23, TRUE))
ASSUME AssertError("Attempted to compute the value of an expression of form\nCHOOSE x \\in S: P, but no element of S satisfied P.\nline 3, col 14 to line 3, col 35 of module JO",
                   NoneOf({1, 2}))
ASSUME AssertError("Applying FoldBag to the following value,\nwhich is not an element of Nat:\n-1 (in: \"b\":>-1)",
                   FoldBag(LAMBDA x, y : x * y, 1, "b" :> -1) = 42)
ASSUME AssertError("Attempted to check if the non-enumerable value\n42\nis element of\nSUBSET 42",
                   MapThenFoldSet(LAMBDA x, y : x - y, 0, LAMBDA i : i, LAMBDA s : Min(s), 42))
ASSUME AssertError("The argument of atoi should be a string, but instead it is:\n\"foo\"", atoi("foo"))
ASSUME AssertError("The argument of atoi should be a string, but instead it is:\n\"2147483648\"", atoi("2147483648"))
ASSUME AssertError("The argument of IsInjective should be a function, but instead it is:\n{}", IsInjective({}))
ASSUME ~AssertError("The argument of atoi", atoi("x"))
ASSUME ~AssertError("anything", 1 + 1)
\* The wrapper of an error inside a Java override (MethodValue.eval) and the
\* errors of the value classes inside it.
ASSUME AssertError("Attempted to apply the operator overridden by the Java method\npublic static tlc2.value.IBoolValue tlc2.module.FiniteSets.IsFiniteSet(tlc2.value.impl.Value),\nbut it produced the following error:\nAttempted to check if the integer 1 is a finite set.",
                   IsFiniteSet([1 -> 2]))
ASSUME AssertError("Attempted to apply the operator overridden by the Java method\npublic static tlc2.value.IBoolValue tlc2.module.FiniteSets.IsFiniteSet(tlc2.value.impl.Value),\nbut it produced the following error:\nAttempted to check if the string \"s\" is a finite set.",
                   IsFiniteSet(Seq("s")))
ASSUME AssertError("Attempted to apply the operator overridden by the Java method\npublic static tlc2.value.IBoolValue tlc2.module.FiniteSets.IsFiniteSet(tlc2.value.impl.Value),\nbut it produced the following error:\nAttempted to check if expression of form {x \\in S : p(x)} is a finite set, but cannot check if S:\nNat\nis finite.",
                   IsFiniteSet({x \in Nat : x < 5}))
\* Cardinality refuses a non-enumerable argument itself (not wrapped) ...
ASSUME AssertError("Attempted to compute cardinality of the value\nSeq({})",
                   Cardinality(Seq({})) = 1)
\* ... and wraps the errors of the value classes.
ASSUME AssertError("Attempted to apply the operator overridden by the Java method\npublic static tlc2.value.impl.IntValue tlc2.module.FiniteSets.Cardinality(tlc2.value.impl.Value),\nbut it produced the following error:\nAttempted to compute the number of elements in the overridden value Seq({}).",
                   Cardinality(Seq({}) \X {1}) = 1)
ASSUME AssertError("Attempted to apply the operator overridden by the Java method\npublic static tlc2.value.impl.IntValue tlc2.module.FiniteSets.Cardinality(tlc2.value.impl.Value),\nbut it produced the following error:\nAttempted to enumerate { x \\in S : p(x) } when S:\nNat\nis not enumerable",
                   Cardinality({x \in Nat : x < 5}) = 5)
ASSUME AssertError("Attempted to apply the operator overridden by the Java method\npublic static tlc2.value.impl.IntValue tlc2.module.FiniteSets.Cardinality(tlc2.value.impl.Value),\nbut it produced the following error:\nSize of interval value exceeds the maximum representable size (32bits): 0..2147483647.",
                   Cardinality(0..2147483647) = 0)
\* A set enumeration is normalized when first used, here inside Cardinality.
ASSUME AssertError("Attempted to apply the operator overridden by the Java method\npublic static tlc2.value.impl.IntValue tlc2.module.FiniteSets.Cardinality(tlc2.value.impl.Value),\nbut it produced the following error:\nAttempted to enumerate S \\cup T when S:\n{0}\nand T:\nNat\nare not both enumerable",
                   Cardinality({Nat, Nat \cup {0}}) = 1)
ASSUME AssertError("Attempted to apply the operator overridden by the Java method\npublic static tlc2.value.impl.Value tlc2.module.TLC.ToString(tlc2.value.impl.Value),\nbut it produced the following error:\nAttempted to compute the number of elements in the overridden value Nat.",
                   ToString([n1 : Nat, n2 : {}]) = "{}")
ASSUME AssertError("Attempted to apply the operator overridden by the Java method\npublic static tlc2.value.impl.IntValue tlc2.module.FiniteSets.Cardinality(tlc2.value.impl.Value),\nbut it produced the following error:\nk=32 and n=64",
                   Cardinality(kSubset(32, 1..64)) = 0)
ASSUME AssertError("The second argument of kSubset should be a set, but instead it is:\n42",
                   kSubset(0, 42))
VARIABLE x
Init == x = 0
Next == x' = x
Inv == AssertError("The argument of atoi should be a string, but instead it is:\n<<0>>", atoi(<<x>>))
====
"#;

#[test]
fn assert_error_compares_tlc_messages_in_every_tier() {
    let library = tlx_testkit::library_paths();
    if !library.iter().any(|d| d.join("IOUtils.tla").exists()) {
        tlx_testkit::skip("assert_error_compares_tlc_messages_in_every_tier", "community/IOUtils.tla");
        return;
    }
    let dir = tlx_testkit::TempDir::new("assert_error");
    std::fs::write(dir.join("JO.tla"), ASSERT_ERROR_SPEC).unwrap();
    std::fs::write(dir.join("JO.cfg"), "INIT Init\nNEXT Next\nINVARIANT Inv\n").unwrap();
    for (tier, args, env) in TIERS {
        let (code, out) = run(&dir, args, env, &library);
        assert_eq!(code, 0, "{tier}:\n{out}");
        assert!(out.contains("No error has been found"), "{tier}:\n{out}");
    }
}

#[test]
fn unsupported_java_definitions_fail_in_every_tier() {
    // IOUtils is a community module (not in the embedded stdlib).
    let library = tlx_testkit::library_paths();
    let has_io = library.iter().any(|d| d.join("IOUtils.tla").exists());
    for (module, op, app, code) in CASES {
        if *module == "IOUtils" && !has_io {
            tlx_testkit::skip("unsupported_java_definitions_fail_in_every_tier", "community/IOUtils.tla");
            continue;
        }
        let dir = spec_dir(module, app);
        let want = format!("{module}!{op} is implemented by TLC in Java and is not supported by tlx");
        for (tier, args, env) in TIERS {
            let (got_code, out) = run(&dir, args, env, &library);
            assert_eq!(got_code, *code, "{module}!{op} ({tier}):\n{out}");
            assert!(out.contains(&want), "{module}!{op} ({tier}) did not report {want:?}:\n{out}");
            // The placeholder value must never be what the invariant sees.
            assert!(!out.contains("is violated"), "{module}!{op} ({tier}) evaluated the placeholder:\n{out}");
        }
    }
}
