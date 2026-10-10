---- MODULE STK2 ----
\* Runaway recursion inside an argument of a user operator: the recursion
\* error must not be deferred like other argument errors (that re-evaluated
\* the argument once per level on the way back: exponential time).
EXTENDS Naturals
VARIABLES nxt
Succ(x) == x + 1
Twice(F(_), x) == F(x)
RECURSIVE Dist(_), Walk(_)
Dist(n) == IF n = 0 THEN 0 ELSE Succ(Dist(nxt[n]))
Walk(n) == IF n = 0 THEN 0 ELSE Twice(LAMBDA y : y + 1, Walk(nxt[n]))
Init == nxt = [n \in 1..3 |-> n - 1]
Next == nxt' = [nxt EXCEPT ![1] = 3]
Inv == Walk(2) <= 3 /\ Dist(3) <= 3
====
