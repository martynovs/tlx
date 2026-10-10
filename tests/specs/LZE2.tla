---- MODULE LZE2 ----
\* As LZE1 without the invariant: TLC reports the violation during
\* its search, before the deadlock at y = 8.
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
Cyc == y = 0 /\ x' = (x + 1) % 4 /\ y' = y
Long == x = 0 /\ y < 8 /\ y' = y + 1 /\ x' = x
Next == Cyc \/ Long
Spec == Init /\ [][Next]_vars
Prop == [](x = 2 => [](x # 0))
====
