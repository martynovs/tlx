---- MODULE LX9 ----
\* As LX4, under a VIEW (TLC's liveness algorithm): after
\* TLCSet("exit", TRUE) the temporal properties are checked on the graph of
\* the states expanded (n <= 3), where Prop holds, not on the whole state
\* space (n reaches 10 and stays).
EXTENDS Naturals, TLC
VARIABLES n, p
Init == n = 0 /\ p \in {0, 1}
Next == /\ n < 10
        /\ n' = n + 1
        /\ UNCHANGED p
        /\ IF n = 3 THEN TLCSet("exit", TRUE) ELSE TRUE
Spec == Init /\ [][Next]_<<n, p>> /\ WF_<<n, p>>(Next)
Prop == <>[](n < 8)
View == n
====
