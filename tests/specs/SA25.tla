---- MODULE SA25 ----
\* `Any \cup {1}` is lazy, but comparing it enumerates it and errors.
EXTENDS TLC
VARIABLE x
Init == x = 0
Next == x' = x
Inv == x = 0 /\ (Any \cup {1}) = {1}
====
