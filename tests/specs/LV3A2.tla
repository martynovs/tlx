---- MODULE LV3A2 ----
\* docs/compat.md D-12: after the deadlock at x = 3, TLC still runs
\* its liveness check on that state and reports a violation of Prop too
\* (exit code 11, the deadlock's). The verdict is the first error.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x < 3 /\ x' = x + 1
Spec == Init /\ [][Next]_x
Prop == [](x = 2 => [](x # 3))
====
