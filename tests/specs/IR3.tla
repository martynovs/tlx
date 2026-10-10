---- MODULE IR3 ----
\* Side effects (TLCSet) in an Init that fails in the IR after them
\* (recursion deeper than the compiled unrolling): the interpreter
\* computes the initial states, and the effects happen once (else Inv
\* fails).
EXTENDS Naturals, TLC
VARIABLES x, y, c
RECURSIVE Sum(_)
Sum(n) == IF n = 0 THEN 0 ELSE n + Sum(n - 1)
ASSUME TLCSet(1, 0)
Init == /\ TLCSet(1, TLCGet(1) + 1)
        /\ y = 9
        /\ x = Sum(y) * 0
        /\ c = TLCGet(1)
Next == /\ x < 3
        /\ x' = x + 1
        /\ y' = y
        /\ c' = c + 1
Inv == c = x + 1
====
