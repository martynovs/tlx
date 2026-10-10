---- MODULE NeonInvOutsideLevel ----
\* A state outside the CONSTRAINT is checked against the invariants every
\* time it is generated (TLC): here first at level 2 (holds) and later at
\* level 4 (violated). The invariant reads TLCGet("level"), so the check
\* must not be skipped for an already checked state.
EXTENDS Naturals, TLC
VARIABLES x, y
Init == x = 0 /\ y = 0
Inc == x < 2 /\ x' = x + 1 /\ UNCHANGED y
Flip == y = 0 /\ y' = 1 /\ UNCHANGED x
Jump == x' = 9 /\ y' = 0
Next == Inc \/ Flip \/ Jump
Constraint == x /= 9
Inv == x = 9 => TLCGet("level") < 4
====
