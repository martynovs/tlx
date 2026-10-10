---- MODULE MX1 ----
\* TLCGet("stats") in a simulation is TLC's simulation record: no distinct
\* states (-1), traces and behavior lengths (found by scripts/matrix.py).
EXTENDS Integers, TLC
VARIABLES x
Init == x = 0
A == x < 3 /\ x' = x + 1
B == x = 3 /\ x' = 0
Next == A \/ B
Post == LET s == TLCGet("stats") IN
          /\ s.distinct = -1 /\ s.distinctvalues = -1 /\ s.retries = -1
          /\ s.traces = 3 /\ s.generated = 16 /\ s.behavior.id = 3
          /\ s.levelmean = 5 /\ s.levelvariance = 0 /\ s.worker = 0
====
