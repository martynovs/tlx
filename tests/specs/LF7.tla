---- MODULE LF7 ----
\* An evaluation error in a state predicate of the tableau on the
\* initial state.
EXTENDS Naturals
VARIABLES x
Init == x = 0
Next == x < 4 /\ x' = x + 1
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Prop == <>(10 \div x = 7)
====
