---- MODULE FzEmptySetEqFcn ----
EXTENDS TLC
VARIABLE x
Init == x = {}
Next == (x # (0 :> 3)) /\ UNCHANGED x
====
