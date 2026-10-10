---- MODULE SAL1 ----
\* Fairness of actions outside the next-state relation: ENABLED <<A>>_v is
\* that of the spec (from the enumeration of A's steps). B is always
\* enabled but never taken, so no behavior is fair and the property holds;
\* C is never enabled.
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
Next == x' = (x + 1) % 3 /\ UNCHANGED y
B == y' = 1 - y /\ UNCHANGED x
C == x = 5 /\ x' = 0 /\ y' = y
Spec == Init /\ [][Next]_vars /\ WF_vars(B) /\ WF_vars(C)
Prop == <>(x = 2)
====
