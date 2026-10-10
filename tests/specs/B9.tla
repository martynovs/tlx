---- MODULE B9 ----
EXTENDS Naturals
VARIABLE x
I == INSTANCE M9 WITH N <- 3
Init == x = 0
Next == I!Branch /\ x' = (x + 1) % 2
====
