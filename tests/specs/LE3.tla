---- MODULE LE3 ----
\* An evaluation error in the ENABLED of fairness (an action outside
\* the next-state relation) at x = 2: TLC stops when it has expanded that
\* state (exit 255), before the deadlock at x = 4.
EXTENDS Naturals
VARIABLES x
Init == x = 0
Next == x < 4 /\ x' = x + 1
Bad == x' = 10 \div (x - 2)
Spec == Init /\ [][Next]_x /\ WF_x(Bad)
Prop == <>(x = 4)
====
