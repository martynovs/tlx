---- MODULE FzHeterogeneousMembership ----
EXTENDS Integers, TLC
VARIABLE x
ASSUME PrintT(ToString(0 \in {0, [a |-> -2, b |-> -1]}))
Init == x = 0
Next == x = 1 /\ UNCHANGED x
====
