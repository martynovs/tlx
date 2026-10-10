---- MODULE LZE3 ----
\* As LZE2 without deadlock checking: TLC stops its search at the
\* violation (its counts are those of that point), with a second property
\* that holds.
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
Cyc == y = 0 /\ x' = (x + 1) % 4 /\ y' = y
Long == x = 0 /\ y < 8 /\ y' = y + 1 /\ x' = x
Next == Cyc \/ Long
Spec == Init /\ [][Next]_vars
Holds == [](y = 3 => [](y > 0))
Prop == [](x = 2 => [](x # 0))
====
