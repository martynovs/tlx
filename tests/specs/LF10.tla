---- MODULE LF10 ----
\* Under a VIEW (TLC's algorithm on concrete states): fairness of an
\* action outside the next-state relation.
EXTENDS Naturals
VARIABLES x, t
Init == x = 0 /\ t = 0
Next == x < 3 /\ x' = x + 1 /\ t' = t + 1
Reset == x = 3 /\ x' = 0 /\ t' = t
Spec == Init /\ [][Next]_<<x, t>> /\ WF_<<x, t>>(Reset)
Prop == <>[](x # 3)
View == x
====
