---- MODULE FZ6 ----
\* Found by scripts/fuzz_expr.py (compiled tier): projecting a component of a
\* tuple or record literal must not skip the other components, whose errors
\* TLC reports (it evaluates the whole literal).
EXTENDS Integers, Sequences
VARIABLES v, r
Pair(a, b) == <<a, b>>
Init == v = 1 /\ r = 0
Next == /\ r < 2
        /\ r' = IF r = 0 THEN [a |-> v, b |-> v + 1].a
                ELSE Pair(v + 1, CHOOSE x \in {} : TRUE)[1]
        /\ v' = v
====
