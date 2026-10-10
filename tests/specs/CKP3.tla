---- MODULE CKP3 ----
\* Checkpoints taken while searching change nothing: the TLCSet
\* register counts the states expanded (the POSTCONDITION checks the
\* count), and y changes shape late (a layout miss of a compiled run after
\* its checkpoints, with TLX_PRERUN=1).
EXTENDS Naturals, TLC
VARIABLES x, y, z
Init == x = 0 /\ y = 0 /\ z = 0 /\ TLCSet(1, 0)
Next == /\ TLCSet(1, TLCGet(1) + 1)
        /\ \/ x < 60 /\ x' = x + 1 /\ z' = z /\ y' = IF x' > 45 THEN <<"late", x'>> ELSE y
           \/ z < 30 /\ z' = z + 1 /\ x' = x /\ y' = y
Post == TLCGet(1) = 61 * 31
====
