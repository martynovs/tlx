---- MODULE LF1 ----
\* Fairness of an action outside the next-state relation. TLC
\* evaluates ENABLED <<Reset>>_x on the state (true at x = 3), so stuttering
\* at 3 is unfair; the steps of the behavior graph never take Reset.
EXTENDS Naturals
VARIABLES x
Init == x = 0
Next == x < 3 /\ x' = x + 1
Reset == x = 3 /\ x' = 0
Spec == Init /\ [][Next]_x /\ WF_x(Reset)
Prop == <>[](x # 3)
====
