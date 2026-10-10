---- MODULE NeonInvOutsideOk ----
\* As NeonInvOutside, without a violation: the counts include every
\* generation of the states outside the constraint.
EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y = 0
Next == \/ x < 6 /\ x' = x + 1 /\ UNCHANGED y
        \/ y < 6 /\ y' = y + 1 /\ UNCHANGED x
        \/ x' = 9 /\ y' = y % 3
Constraint == x /= 9
Inv == x = 9 => y < 3
====
