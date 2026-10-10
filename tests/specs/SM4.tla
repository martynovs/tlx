---- MODULE SM4 ----
\* ToString, Print and PrintT expand lazy sets and functions like TLC's
\* Value.toStringUnchecked: a set filter over a non-enumerable value cannot
\* be printed (an error), while Nat, Seq(S) and large sets print
\* symbolically. The error happens in the second state.
EXTENDS Naturals, Sequences, TLC
VARIABLES b
Init == b = TRUE
Next == b' = FALSE
S == IF b THEN {1, 2} ELSE FALSE
Inv == /\ ToString(Seq({1})) = "Seq({1})"
       /\ ToString(Nat) = "Nat"
       /\ PrintT(<<"ok", SUBSET (1..10), [1..20 -> 1..20]>>)
       /\ ToString({x \in S : TRUE}) # ""
====
