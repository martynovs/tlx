---- MODULE B10 ----
EXTENDS Naturals
VARIABLE x
Pick == [s \in SUBSET {1, 2} |-> CHOOSE e \in s : TRUE]
Init == x = 0
Next == x' = Pick[{1, 2}]
====
