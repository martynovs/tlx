---- MODULE LS5 ----
\* Run-time strings are ordered by creation. The search
\* creates "qx", "qxx", then "qy", so the CHOOSE picks "qxx" and n never
\* becomes 99999 (TLC: no error). The compiled tier folded the constant
\* concatenations at compile time in source order ("qy" first) and the
\* CHOOSE picked "qy". The strings come after the shape pre-run's budget
\* (20000 states), which would otherwise create them first, in order.
EXTENDS Naturals, Sequences, TLC
N == 30000
VARIABLES n, s
Init == n = 0 /\ s = ""
Next == /\ n < N + 5
        /\ n' = IF n = N + 3 /\ (CHOOSE t \in {"q" \o "y", ("q" \o "x") \o "x"} : TRUE) = "q" \o "y" THEN 99999 ELSE n + 1
        /\ s' = CASE n = N -> "q" \o "x"
                  [] n = N + 1 -> s \o "x"
                  [] n = N + 2 -> "q" \o "y"
                  [] OTHER -> s
Inv == n # 99999
====
