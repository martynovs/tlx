---- MODULE SPL2 ----
\* A liveness violation found on a behavior graph whose edge logs
\* are spilled and read back (SPL2.flags: TLX_SPILL=always, 4 KiB blocks).
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
IncX == x' = (x + 1) % 40 /\ y' = y
IncY == y' = (y + 1) % 40 /\ x' = x
Next == IncX \/ IncY
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
Prop == <>(x = 39 /\ y = 39)
====
