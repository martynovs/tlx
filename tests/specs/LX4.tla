---- MODULE LX4 ----
\* After TLCSet("exit", TRUE) TLC checks the temporal properties
\* on the graph of the states it expanded (n <= 3; n = 4 is not expanded:
\* no behavior goes on from it, not even by stuttering). Prop holds there
\* (WF forbids stuttering at n <= 3), not on the whole state space (n
\* reaches 10 and stays).
EXTENDS Naturals, TLC
VARIABLES n
Init == n = 0
Next == /\ n < 10
        /\ n' = n + 1
        /\ IF n = 3 THEN TLCSet("exit", TRUE) ELSE TRUE
Spec == Init /\ [][Next]_n /\ WF_n(Next)
Prop == <>[](n < 8)
====
