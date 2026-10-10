---- MODULE FzConstBoxProp ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x < 2 /\ x' = x + 1
Spec == Init /\ [][Next]_x
Prop == [](FALSE)
====
