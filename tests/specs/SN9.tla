---- MODULE SN9 ----
\* A typed model value (x_a) compared with a string is a TLC error,
\* also when both are atoms of the compiled tier (equality in a common
\* encoding; SN10: membership). The small pre-run (SN9.flags) sees strings
\* only.
EXTENDS Naturals
CONSTANT x_a
VARIABLES n, a, b
Init == n = 0 /\ a = "s" /\ b = "t"
Next == /\ n < 5
        /\ n' = n + 1
        /\ a' = a
        /\ b' = IF n = 2 THEN x_a ELSE b
Inv == a # b
====
