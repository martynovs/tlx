---- MODULE LX2 ----
\* TLCSet("exit", TRUE) stops the search after the state whose
\* expansion requested it (TLC: 5 states). The compiled tier's pre-run also
\* evaluates it: its request must not stop the search before it starts.
EXTENDS Naturals, TLC
VARIABLES n
Init == n = 0
Next == /\ n < 10
        /\ n' = n + 1
        /\ IF n = 3 THEN TLCSet("exit", TRUE) ELSE TRUE
Spec == Init /\ [][Next]_n
====
