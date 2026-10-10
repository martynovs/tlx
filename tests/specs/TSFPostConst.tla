---- MODULE TSFPostConst ----
\* Contract: a POSTCONDITION that is a constant-level definition is an
\* error after the search (TLC has its value instead of a definition:
\* "The post condition Post cannot be a constant."), whatever its value.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x < 3 /\ x' = x + 1
Post == 1 + 1 = 2
====
