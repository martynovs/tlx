---- MODULE SPL3 ----
\* A temporal property that holds, checked on a behavior graph whose
\* edge logs are spilled and read back (SPL3.flags: TLX_SPILL=always).
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
IncX == x' = (x + 1) % 40 /\ y' = y
IncY == y' = (y + 1) % 40 /\ x' = x
Next == IncX \/ IncY
Spec == Init /\ [][Next]_vars /\ WF_vars(IncX)
Prop == []<>(x = 0)
====
