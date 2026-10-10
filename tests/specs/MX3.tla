---- MODULE MX3 ----
\* TLC's simulator checks the POSTCONDITION after an error too (found by
\* scripts/matrix.py).
EXTENDS Naturals, TLC
VARIABLES x
Init == x = 0
Next == x' = x + 1 /\ (x = 2 => 1 \div 0 = 0)
Post == TLCGet("stats").generated > 100
====
