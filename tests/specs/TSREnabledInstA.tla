---- MODULE TSREnabledInstA ----
\* Instantiated by TSREnabledInst and TSREnabledInst2.
EXTENDS Naturals
VARIABLES x, y
F == ENABLED (x' = 0 /\ y' = 1)
G == ENABLED ((x # y) /\ (x' = 0) /\ (y' = 1))
A == (x' = x) /\ (y' = y + 1)
B(d) == ENABLED d
C == B(A)
D == ENABLED A
====
