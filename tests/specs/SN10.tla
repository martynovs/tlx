---- MODULE SN10 ----
\* A typed model value (x_a) compared with a string is a TLC error,
\* also in a membership test in an encoded set of strings (SN9: equality).
\* The small pre-run (SN10.flags) sees strings only.
EXTENDS Naturals
CONSTANT x_a
VARIABLES n, b, s
Init == n = 0 /\ b = "t" /\ s = {"s", "t"}
Next == /\ n < 5
        /\ n' = n + 1
        /\ s' = s
        /\ b' = IF n = 2 THEN x_a ELSE b
Inv == b \in s
====
