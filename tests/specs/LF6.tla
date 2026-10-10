---- MODULE LF6 ----
\* Assert fails in the ENABLED of fairness: the exit status of a
\* failed Assert.
EXTENDS Naturals, TLC
VARIABLES x
Init == x = 0
Next == x < 4 /\ x' = x + 1
Bad == Assert(x # 2, "boom") /\ x' = 0
Spec == Init /\ [][Next]_x /\ WF_x(Bad)
Prop == <>(x = 4)
====
