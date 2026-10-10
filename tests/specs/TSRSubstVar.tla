---- MODULE TSRSubstVar ----
\* The cfg substitutes a definition that refers to a variable for a
\* constant: TLC evaluates it at startup and rejects the configuration.
EXTENDS Naturals
CONSTANT A
VARIABLE v
D == v + 1
Init == v = 0
Next == v' = 1 - v
====
