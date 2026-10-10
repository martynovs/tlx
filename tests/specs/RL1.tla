---- MODULE RL1 ----
\* Reals: TLC has no real numbers; it evaluates the dummy definitions of
\* Reals.tla (Real == "Reals"), so `/` fails (CHOOSE over a string).
EXTENDS Reals, Sequences
VARIABLE x
ASSUME Real = "Reals" /\ Len(Real) = 5
Init == x = 0
Next == x' = 6 / 3
====
