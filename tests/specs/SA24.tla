---- MODULE SA24 ----
\* `S \subseteq T` enumerates S: `Any \subseteq {1}` errors.
EXTENDS TLC
VARIABLE x
Init == x = 0
Next == x' = x
Inv == x = 0 /\ Any \subseteq {1}
====
