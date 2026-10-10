---- MODULE TSXErrTexts ----
\* AssertError compares error messages, so TLC's texts of these errors are
\* observable (TLC's EmptySetEqAssumeTest, ValueSemanticsAssumeTest): the
\* size errors of the value classes, a set operation that converts itself
\* to an enumerated set before comparing, and the quantifier-bound errors
\* of Nat, Seq(S) and STRING (with the location of the bound). (The
\* messages TLC builds from its message table, such as the wrapper of an
\* error inside a Java override, carry the markers of -tool, with which
\* the spec tests run TLC: those are checked by
\* crates/tlx-cli/tests/java_only.rs.)
EXTENDS FiniteSets, Integers, Sequences, TLC, TLCExt

\* A union, intersection, set filter or UNION converts itself first.
ASSUME AssertError("Attempted to enumerate S \\cup T when S:\n{0}\nand T:\nNat\nare not both enumerable",
                   Nat \cup {0} = Nat)
ASSUME AssertError("Attempted to enumerate S \\cap T when neither S:\nNat\nnor T:\nNat\nis enumerable",
                   Nat \cap Nat = Nat)
ASSUME AssertError("Attempted to enumerate { x \\in S : p(x) } when S:\nNat\nis not enumerable",
                   {x \in Nat : TRUE} = Nat)
ASSUME AssertError("Attempted to enumerate UNION(s), but some element of s is nonenumerable.",
                   UNION {Nat} = Nat)
ASSUME AssertError("Attempted to enumerate { x \\in S : p(x) } when S:\nNat\nis not enumerable",
                   \E x \in {x \in Nat : x < 5} : x = 4)
\* Functions over an overridden domain are sized when converted.
ASSUME AssertError("Attempted to compute the number of elements in the overridden value Nat.",
                   [x \in Nat |-> 0] \in [Nat -> {0}])
ASSUME AssertError("Attempted to compute the number of elements in the overridden value STRING.",
                   [x \in STRING |-> 0] \in [STRING -> {0}])
ASSUME AssertError("Attempted to compute the number of elements in the overridden value Nat.",
                   <<>> \notin [(SUBSET Nat) -> {}])
\* Quantifier bounds.
ASSUME AssertError("TLC encountered the non-enumerable quantifier bound\nSeq({})\nline 33, col 29 to line 33, col 35 of module TSXErrTexts\nIn TLA+, Seq(S) represents the set of all finite sequences whose elements come from the set S. Even when S\nis a finite set, the number of possible sequences in Seq(S) is unbounded because sequences can have any\nfinite length (e.g., length 0, 1, 2, and so on). As a result, TLC cannot evaluate expressions that\nuniversally (\\A) or existentially (\\E) quantify over Seq({}), because this would require checking an\ninfinite number of cases. Note that for a finite set of sequences s, TLC handles s \\subseteq Seq(S).\nSee https://explain.tlapl.us/seq-unenumerable for additional details.",
                   \E s \in Seq({}) : s = <<>>)
ASSUME AssertError("TLC encountered the non-enumerable quantifier bound\nNat\nline 35, col 29 to line 35, col 31 of module TSXErrTexts\nThe set Nat contains infinitely many elements. As a result, TLC cannot evaluate expressions that\nuniversally (\\A) or existentially (\\E) quantify over Nat, because this would require checking an\ninfinite number of cases. Note that TLC handles set membership like T \\subseteq Nat for any finite set T.",
                   \A n \in Nat : n >= 0)
ASSUME AssertError("TLC encountered a non-enumerable quantifier bound\nSTRING.\nline 37, col 29 to line 37, col 34 of module TSXErrTexts",
                   \E s \in STRING : s = "a")

VARIABLE v
Init == v = 0
Next == UNCHANGED v
====
