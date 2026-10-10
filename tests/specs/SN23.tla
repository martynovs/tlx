---- MODULE SN23 ----
\* i \in DOMAIN f[k] with a domain fixed by the shape of f[k] is a range
\* test, but f[k] is still evaluated: k outside DOMAIN f is a TLC error.
EXTENDS Integers
CONSTANTS a, b, c
VARIABLES n, f, k
Init == n = 0 /\ f = [x \in {a, b} |-> <<1, 2>>] /\ k = a
Next == /\ n < 3
        /\ n' = n + 1
        /\ f' = f
        /\ k' = IF n = 1 THEN c ELSE b
Inv == 2 \in DOMAIN f[k]
====
