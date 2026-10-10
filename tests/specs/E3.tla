---- MODULE E3 ----
\* TLC: a PROPERTY cannot be (or contain a conjunct that is) an action.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Prop == x \in 0..2 /\ x' # x
====
