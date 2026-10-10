---- MODULE LZE4 ----
\* As LZE1, but the invariant is violated (y = 2) before TLC can
\* report the temporal property during its search: the invariant wins.
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
Cyc == y = 0 /\ x' = (x + 1) % 4 /\ y' = y
Long == x = 0 /\ y < 8 /\ y' = y + 1 /\ x' = x
Next == Cyc \/ Long
Spec == Init /\ [][Next]_vars
Inv == y < 2
Prop == [](x = 2 => [](x # 0))
====
