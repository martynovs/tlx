---- MODULE SQS1 ----
\* Wide levels (up to 6,000 states) whose queue segments go to disk
\* and are read back ahead of the claims, and layout misses (h grows)
\* resumed from the spilled archive (SQS1.flags: TLX_SPILL=always, 4 KiB
\* segments, TLX_PRERUN=1).
EXTENDS Naturals, Sequences
VARIABLES a, b, c, d, e, h
Init == a = 0 /\ b = 0 /\ c = 0 /\ d = 0 /\ e = 0 /\ h = <<>>
Next == \/ a < 9 /\ a' = a + 1 /\ UNCHANGED <<b, c, d, e, h>>
        \/ b < 9 /\ b' = b + 1 /\ UNCHANGED <<a, c, d, e, h>>
        \/ c < 9 /\ c' = c + 1 /\ UNCHANGED <<a, b, d, e, h>>
        \/ d < 9 /\ d' = d + 1 /\ UNCHANGED <<a, b, c, e, h>>
        \/ e < 9 /\ e' = e + 1 /\ h' = Append(h, 100 * e) /\ UNCHANGED <<a, b, c, d>>
        \/ a = 9 /\ b > 0 /\ a' = 0 /\ b' = b - 1 /\ UNCHANGED <<c, d, e, h>>
TypeOK == a \in 0..9 /\ b \in 0..9 /\ c \in 0..9 /\ d \in 0..9 /\ e \in 0..9 /\ Len(h) = e
====
