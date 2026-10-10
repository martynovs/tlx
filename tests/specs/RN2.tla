---- MODULE RN2 ----
\* TLC evaluates 0-ary constant definitions once, at startup, even when they
\* are nondeterministic (RandomElement, RandomSubset, Print): every use sees
\* the same value. TLCGet("level") fails at startup and is re-evaluated.
EXTENDS Integers, TLC, Randomization, FiniteSets
VARIABLES x, y, z, w
R == RandomElement(1..1000000)
S == RandomSubset(3, 1..1000)
L == TLCGet("level")
P == Print("P evaluated", 1)
Init == x = 0 /\ y = {} /\ z = 0 /\ w = 0
Next == /\ x < 5
        /\ x' = x + 1
        /\ y' = y \cup {<<R, S>>}
        /\ z' = L
        /\ w' = w + P
Inv == /\ Cardinality(y) <= 1
       /\ z = x
====
