---- MODULE IR2 ----
\* Side effects (TLCSet) in an action that fails in the IR after them
\* (recursion deeper than the compiled unrolling): the interpreter
\* evaluates the action, and the effects happen once (else Inv fails).
EXTENDS Naturals, TLC
VARIABLES x, y, c
RECURSIVE Sum(_)
Sum(n) == IF n = 0 THEN 0 ELSE n + Sum(n - 1)
ASSUME TLCSet(1, 0)
Init == /\ TLCSet(1, TLCGet(1) + 1)
        /\ y = 9
        /\ x = 0
        /\ c = TLCGet(1)
Next == /\ x < 3
        /\ TLCSet(1, TLCGet(1) + 1)
        /\ x' = x + 1 + 0 * Sum(y)
        /\ y' = y
        /\ c' = TLCGet(1)
Inv == c = x + 1
====
