---- MODULE FzHeterogeneousUnion ----
EXTENDS Integers, FiniteSets, Sequences
VARIABLE x
R == [a |-> 1, b |-> 2]
S == {R, <<1>>}
T == {R}
Init == x = 0
Next == x' = Cardinality(S \cup T)
Spec == Init /\ [][Next]_x
====
