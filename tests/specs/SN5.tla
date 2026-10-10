---- MODULE SN5 ----
\* A function built over the interval 1..2 is a tuple for EXCEPT: a
\* model-value key is an error (TLC: "Attempted to apply function with
\* integer domain to the non-integer argument"), unlike the same function
\* built over the enumerated set {1, 2} (SN3).
EXTENDS Naturals
CONSTANTS M
VARIABLES n, f
Init == n = 0 /\ f = [k \in 1..2 |-> 0]
Next == /\ n < 2
        /\ n' = n + 1
        /\ f' = IF n = 0 THEN [f EXCEPT ![1] = 1] ELSE [f EXCEPT ![M] = 2]
====
