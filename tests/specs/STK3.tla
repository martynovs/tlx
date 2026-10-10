---- MODULE STK3 ----
\* Runaway recursion in an action guard, reached after two steps (the
\* compiled tier's pre-run and native code meet it too).
EXTENDS Naturals
VARIABLES nxt, c
RECURSIVE Dist(_)
Dist(n) == IF n = 0 THEN 0 ELSE 1 + Dist(nxt[n])
Init == nxt = [n \in 1..3 |-> n - 1] /\ c = 0
Next == /\ c < 3
        /\ Dist(3) < 10
        /\ c' = c + 1
        /\ nxt' = IF c = 1 THEN [nxt EXCEPT ![1] = 3] ELSE nxt
Inv == TRUE
====
