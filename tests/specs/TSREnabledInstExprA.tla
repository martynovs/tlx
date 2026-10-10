---- MODULE TSREnabledInstExprA ----
\* Instantiated by TSREnabledInstExpr.
EXTENDS Naturals
VARIABLES a, b
P == ENABLED (a' = 5)
Q == ENABLED (a' = a + 1)
R == ENABLED (b' = <<7, 8>> /\ a' = 3)
S == ENABLED (b' = <<b[1] + 1, b[2]>>)
T == ENABLED (a' \in {1, 2} /\ a' > 1 /\ UNCHANGED b)
====
