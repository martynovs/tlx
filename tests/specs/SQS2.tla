---- MODULE SQS2 ----
\* A deep invariant violation after wide levels whose queue segments
\* went to disk (SQS2.flags: TLX_SPILL=always, 4 KiB segments).
EXTENDS Naturals
VARIABLES a, b, c, d, e
Init == a = 0 /\ b = 0 /\ c = 0 /\ d = 0 /\ e = 0
Next == \/ a < 9 /\ a' = a + 1 /\ UNCHANGED <<b, c, d, e>>
        \/ b < 9 /\ b' = b + 1 /\ UNCHANGED <<a, c, d, e>>
        \/ c < 9 /\ c' = c + 1 /\ UNCHANGED <<a, b, d, e>>
        \/ d < 9 /\ d' = d + 1 /\ UNCHANGED <<a, b, c, e>>
        \/ e < 9 /\ e' = e + 1 /\ UNCHANGED <<a, b, c, d>>
Inv == ~(a = 9 /\ b = 8 /\ c = 9 /\ d = 7 /\ e = 6)
====
