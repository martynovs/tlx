---- MODULE EK5 ----
\* An invariant that cannot be evaluated in a reachable (non-initial)
\* state: "Evaluating invariant Inv failed." (2111, exit 76) in every tier,
\* also when the invariant is compiled.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 5
Inv == <<1, 2>>[x + 1] > 0
====
