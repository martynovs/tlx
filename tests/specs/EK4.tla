---- MODULE EK4 ----
\* Evaluation errors while computing successors, by kind (TLC reports each
\* with its own message code and exit status): a general error (K = 1),
\* Assert (K = 2), division by zero (K = 3); and an invariant that cannot
\* be evaluated (K = 4).
EXTENDS Integers, TLC
CONSTANT K
VARIABLE x
f == [i \in 1..2 |-> i]
Init == x = 0
Next == /\ x < 2
        /\ x' = x + 1
        /\ \/ K \notin 1..3
           \/ K = 1 /\ f[3] = 1
           \/ K = 2 /\ Assert(x = 0, "boom")
           \/ K = 3 /\ 3 \div x > 0
Inv == K # 4 \/ 6 \div (1 - x) > -10
====
