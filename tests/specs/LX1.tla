---- MODULE LX1 ----
\* After TLCSet("exit", TRUE) TLC checks the temporal properties
\* on the graph of the states it expanded (n <= 3): without fairness a
\* behavior stutters at some n <= 3, so Prop is violated there.
EXTENDS Naturals, TLC
VARIABLES n
Init == n = 0
Next == /\ n < 10
        /\ n' = n + 1
        /\ IF n = 3 THEN TLCSet("exit", TRUE) ELSE TRUE
Spec == Init /\ [][Next]_n
Prop == <>(n = 10)
====
