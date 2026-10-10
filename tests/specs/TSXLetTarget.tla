---- MODULE TSXLetTarget ----
\* A primed variable under LET on the left of = is an assignment, as TLC's
\* getPrimedVar / getVar look through LET (Github725d): in Init, Next and
\* ENABLED.
EXTENDS Naturals
VARIABLE a, b
Init == (LET y == a IN y) = 0 /\ (LET z == b IN z) \in {0, 1}
Next == (LET y == a' IN y) = (a + 1) % 3 /\ (LET z == b' IN LET w == z IN w) \in {b, 1 - b}
Inv == ENABLED ((LET y == a' IN y) = 0)
====
