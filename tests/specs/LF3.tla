---- MODULE LF3 ----
\* Fairness of an action that leaves a variable of its
\* subscript unspecified: TLC's ENABLED fails ("does not specify the primed
\* value") at the first state it is evaluated on, during the search.
EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y = 0
Inc == x < 2 /\ x' = x + 1
Next == Inc /\ UNCHANGED y
Spec == Init /\ [][Next]_<<x, y>> /\ WF_<<x, y>>(Inc)
Prop == <>(x = 2)
====
