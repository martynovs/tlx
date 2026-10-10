---- MODULE CG1 ----
\* SequencesExt!Contains is overridden by a Java method in TLC, which
\* evaluates both arguments even when the sequence is empty (the TLA+
\* definition does not use `e` then). The compiled invariant inlined the
\* definition and skipped the failing argument; TLC (and tlx) report the
\* error in state 3.
EXTENDS Integers, SequencesExt
VARIABLE x, s
Init == x = 0 /\ s = <<>>
Next == x < 3 /\ x' = x + 1 /\ UNCHANGED s
Inv == ~Contains(s, IF x = 2 THEN CHOOSE n \in {} : TRUE ELSE 1)
====
