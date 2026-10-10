---- MODULE LV3A1 ----
\* docs/compat.md D-12: Bad's successor of <<3, 0>> violates Inv;
\* TLC then still runs its liveness check on <<3, 0>>, whose successor
\* <<0, 0>> closes the cycle: it reports a violation of Prop too (exit code
\* 12, the invariant's). The verdict is the first error.
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
Cyc == y = 0 /\ x' = (x + 1) % 4 /\ y' = y
Bad == x = 3 /\ y = 0 /\ y' = 9 /\ x' = x
Next == Cyc \/ Bad
Spec == Init /\ [][Next]_vars
Inv == y # 9
Prop == [](x = 2 => [](x # 0))
====
