---- MODULE LX10 ----
\* TLC evaluates the liveness predicates during its search and stops
\* at the error in Q (n = 2: 4 states). tlx finds it after the search, then
\* searches again to stop where TLC does: that search must not start with
\* the first one's TLCSet("exit", TRUE) still requested (it stopped at once
\* and the error kept the first search's counts, 5 states).
EXTENDS Naturals, TLC
VARIABLES n
Init == n = 0
Next == /\ n < 10
        /\ n' = n + 1
        /\ IF n = 3 THEN TLCSet("exit", TRUE) ELSE TRUE
Spec == Init /\ [][Next]_n /\ WF_n(Next)
Q == IF n = 2 THEN 1 = TRUE ELSE TRUE
Prop == []<>Q
====
