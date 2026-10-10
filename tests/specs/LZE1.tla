---- MODULE LZE1 ----
\* TLC reports a violation of a "safety-like" temporal property
\* during its search (LiveCheck.TableauLiveChecker: the cycle x = 0..3 closes
\* on a state already expanded and reaches the accepting sink of the
\* tableau), before the invariant violated deeper in the BFS (y = 7).
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
Cyc == y = 0 /\ x' = (x + 1) % 4 /\ y' = y
Long == x = 0 /\ y < 8 /\ y' = y + 1 /\ x' = x
Next == Cyc \/ Long
Spec == Init /\ [][Next]_vars
Inv == y < 7
Prop == [](x = 2 => [](x # 0))
====
