---- MODULE SAL2 ----
\* Fairness of actions outside the next-state relation that are never
\* enabled (C), or enabled without changing their subscript (E: x is
\* unchanged, so <<E>>_x is never enabled): the fairness holds trivially and
\* stuttering at x = 0 violates the property.
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
Next == x' = (x + 1) % 3 /\ UNCHANGED y
C == x = 5 /\ x' = 0 /\ y' = y
E == x' = x /\ y' \in {y, 1 - y}
Spec == Init /\ [][Next]_vars /\ WF_vars(C) /\ WF_x(E)
Prop == <>(x = 2)
====
