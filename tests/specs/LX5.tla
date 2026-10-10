---- MODULE LX5 ----
\* TLCSet("exit", TRUE) stops the search before the next state
\* (TLC ModelChecker.stop ends the queue): the state whose expansion
\* requested it still gets all its successors (A's n = 4, then B's n = 23:
\* 9 states). So B stays enabled at n = 3 and WF_n(B) forbids stuttering
\* there: Prop holds on the graph of the expanded states.
EXTENDS Naturals, TLC
VARIABLES n
Init == n = 0
A == n < 10 /\ n' = n + 1 /\ IF n = 3 THEN TLCSet("exit", TRUE) ELSE TRUE
B == n < 10 /\ n' = n + 20
Next == A \/ B
Spec == Init /\ [][Next]_n /\ WF_n(B)
Prop == <>(n >= 20)
====
