---- MODULE LZS2 ----
\* As LZS1 without fairness, for <>[](x = 1): TLC's simulator
\* (LiveCheck1) sees x = 0 at the node of x = 1 and reports a violation by
\* the behavior stuttering at x = 1, which violates no property: "Temporal
\* properties were violated." without names.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x = 0 /\ x' = 1
Spec == Init /\ [][Next]_x
Prop == <>[](x = 1)
====
