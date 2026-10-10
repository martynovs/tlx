---- MODULE SN21 ----
\* A function on 0..1 (Map shape) and a function on two strings in a
\* membership test: D-31 deliberately treats different domains as unequal.
\* TLC compares an integer with a string and fails at the equal-size pair;
\* tlx continues to the genuine deadlock (5 generated/distinct, depth 5).
EXTENDS Naturals
VARIABLES n, f
Init == n = 0 /\ f = <<>>
Next == /\ n < 4
        /\ n' = n + 1
        /\ f' = [i \in 0..n |-> n]
Inv == f \notin {<<>>, [i \in {"a", "b"} |-> 0]} \/ n = 0
====
