---- MODULE AS2 ----
\* An assumption that cannot be evaluated: TLC exits with
\* VIOLATION_ASSUMPTION (10), like a false assumption.
EXTENDS Integers
VARIABLE x
ASSUME (CHOOSE i \in {} : TRUE) = 1
Init == x = 0
Next == UNCHANGED x
====
