---- MODULE LX11 ----
\* After TLCSet("exit", TRUE) at n = 3, an atom that fails only on a
\* state the search never expanded (n = 7) is never evaluated: TLC finds no
\* error and checks Prop on the graph of the expanded states, where it
\* holds (WF forbids stuttering at n <= 3).
EXTENDS Naturals, TLC
VARIABLES n
Init == n = 0
Next == /\ n < 10
        /\ n' = n + 1
        /\ IF n = 3 THEN TLCSet("exit", TRUE) ELSE TRUE
Spec == Init /\ [][Next]_n /\ WF_n(Next)
Prop == <>(n = 4 \/ 10 \div (n - 7) = 99)
====
