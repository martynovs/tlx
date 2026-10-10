---- MODULE LF5 ----
\* An evaluation error in a state predicate of the tableau at x = 2:
\* TLC evaluates it on the successors of the state it expands, so the
\* behavior ends at x = 1.
EXTENDS Naturals
VARIABLES x
Init == x = 0
Next == x < 4 /\ x' = x + 1
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Prop == <>(10 \div (x - 2) = 7)
====
