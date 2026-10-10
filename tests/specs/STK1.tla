---- MODULE STK1 ----
\* A reachable pointer cycle makes a RECURSIVE operator recurse forever
\* through a function application: an evaluation error (TLC: a stack
\* overflow), not a crash, on every platform and tier.
EXTENDS Naturals
VARIABLES nxt
RECURSIVE Dist(_)
Dist(n) == IF n = 0 THEN 0 ELSE 1 + Dist(nxt[n])
Init == nxt = [n \in 1..3 |-> n - 1]
Next == nxt' = [nxt EXCEPT ![1] = 3]
Inv == Dist(3) <= 3
====
