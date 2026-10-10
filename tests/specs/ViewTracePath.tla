---- MODULE ViewTracePath ----
EXTENDS Naturals, Sequences, TLCExt
VARIABLE x
Init == x = 0
Next == x' = x + 1
Spec == Init /\ [][Next]_x
Inv == Len(Trace) < 3
View == x
====
