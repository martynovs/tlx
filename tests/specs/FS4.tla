---- MODULE FS4 ----
\* FoldLeft on a record (not a 1..n sequence).
EXTENDS SequencesExt
ASSUME \E z \in {FoldLeft(LAMBDA p, q : p + q, 0, [a |-> 1])} : TRUE
VARIABLE x
Init == x = 0
Next == x' = x
====
