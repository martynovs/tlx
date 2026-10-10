---- MODULE LZE5 ----
\* As LZE1 under a VIEW that ignores w (several states per
\* fingerprint): TLC still reports the temporal property during its search.
EXTENDS Naturals
VARIABLES x, y, w
vars == <<x, y, w>>
Init == x = 0 /\ y = 0 /\ w = 0
Cyc == y = 0 /\ x' = (x + 1) % 4 /\ y' = y /\ w' = 1 - w
Long == x = 0 /\ y < 8 /\ y' = y + 1 /\ x' = x /\ w' = w
Next == Cyc \/ Long
Spec == Init /\ [][Next]_vars
Inv == y < 7
Prop == [](x = 2 => [](x # 0))
View == <<x, y>>
====
