---- MODULE LZF2 ----
\* As LZF1, for Live2 == <>(y = 1): violated by the (fair) stuttering
\* at x = 2, y = 0.
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
A == x < 2 /\ x' = x + 1 /\ y' = y
B == y' = 1 - y /\ x' = x
Next == A \/ B
Spec == Init /\ [][Next]_vars /\ WF_x(A \/ B)
Live2 == <>(y = 1)
====
