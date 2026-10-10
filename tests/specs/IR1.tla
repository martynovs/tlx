---- MODULE IR1 ----
\* An Init that the IR cannot run (recursion deeper than the compiled
\* unrolling): the interpreter computes the initial states instead.
EXTENDS Naturals
VARIABLES x, y
RECURSIVE Sum(_)
Sum(n) == IF n = 0 THEN 0 ELSE n + Sum(n - 1)
Init == /\ y \in 9..10
        /\ x = Sum(y)
Next == /\ x < Sum(y) + 3
        /\ x' = x + 1
        /\ y' = y
====
