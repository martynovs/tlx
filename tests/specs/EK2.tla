---- MODULE EK2 ----
\* The VIEW fails on the second initial state: it still counts as generated.
EXTENDS Naturals
VARIABLE x
Init == x \in {0, 1}
Next == UNCHANGED x
V == 10 \div (1 - x)
====
