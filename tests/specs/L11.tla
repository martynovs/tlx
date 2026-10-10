---- MODULE L11 ----
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
IncX == x < 2 /\ x' = x + 1 /\ UNCHANGED y
IncY == y < 2 /\ y' = y + 1 /\ UNCHANGED x
Next == IncX \/ IncY
Fair(v) == WF_v(IncX) /\ WF_v(IncY)
Spec == Init /\ [][Next]_vars /\ Fair(vars)
Prop == <>[](~ENABLED IncX) /\ <>(x = 2 /\ y = 2)
====
