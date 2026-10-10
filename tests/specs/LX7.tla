---- MODULE LX7 ----
\* TLCSet("exit", TRUE) in an invariant, on the successor n = 3:
\* TLC expands n <= 2 only and checks the temporal properties there (WF
\* forbids stuttering at n <= 2: no error).
EXTENDS Naturals, TLC
VARIABLES n
Init == n = 0
Next == n < 10 /\ n' = n + 1
Spec == Init /\ [][Next]_n /\ WF_n(Next)
Inv == IF n = 3 THEN TLCSet("exit", TRUE) ELSE TRUE
Prop == <>[](n < 8)
====
