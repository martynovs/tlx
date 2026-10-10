---- MODULE LC10 ----
\* No cycle condition at all: TLC shows the start of the SCC stuttering.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Spec == Init /\ [][Next]_x
P == <>(x = 5)
====
