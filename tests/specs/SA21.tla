---- MODULE SA21 ----
\* TLC's AnySet.compareTo throws: `Any` cannot be compared with anything,
\* not even itself.
EXTENDS TLC
VARIABLE x
Init == x = 0
Next == x' = x
Inv == x = 0 /\ Any = Any
====
