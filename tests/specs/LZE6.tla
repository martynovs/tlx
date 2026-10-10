---- MODULE LZE6 ----
\* As LZE1 with fairness: every disjunct of the negated property then
\* has cycle conditions, TLC finds no violation during its search and
\* reports the invariant.
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
Cyc == y = 0 /\ x' = (x + 1) % 4 /\ y' = y
Long == x = 0 /\ y < 8 /\ y' = y + 1 /\ x' = x
Next == Cyc \/ Long
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
Inv == y < 7
Prop == [](x = 2 => [](x # 0))
====
