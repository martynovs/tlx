---- MODULE NeonInvOutside ----
\* States outside the CONSTRAINT are generated many times and checked
\* against the invariants each time (TLC); the compiled tier skips
\* re-checking a state it already checked without a violation (pure
\* invariants). Here a state outside the constraint violates the
\* invariant, and is first generated late in the search.
EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y = 0
Next == \/ x < 6 /\ x' = x + 1 /\ UNCHANGED y
        \/ y < 6 /\ y' = y + 1 /\ UNCHANGED x
        \/ x' = 9 /\ y' = y % 4
Constraint == x /= 9
Inv == x = 9 => y < 3
====
