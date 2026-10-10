---- MODULE FzHeterogeneousUnionPrint ----
EXTENDS Integers, Sequences, FiniteSets, TLC
VARIABLE x
R == [a |-> 0, b |-> -2]
ASSUME PrintT(ToString({R, <<-1>>} \cup {R}))
Init == x = 0
Next == x = 1 /\ UNCHANGED x
====
