---- MODULE EK3 ----
\* Errors while checking an initial state: an invariant that cannot be
\* evaluated (a general error: "While working on the initial state"; one of
\* a specific kind, division by zero, is reported as such).
EXTENDS Integers
CONSTANT K
VARIABLE x
f == [i \in 1..2 |-> i]
Init == x = 0
Next == UNCHANGED x
Inv == IF K = 1 THEN f[3] = 1 ELSE 6 \div x > 0
====
