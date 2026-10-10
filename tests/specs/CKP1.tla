---- MODULE CKP1 ----
\* Checkpoints taken while searching (TLX_CHECKPOINT_STATES) change
\* nothing: strings built at run time, a liveness property violated (the
\* edge logs are written to the checkpoints).
EXTENDS Naturals, Sequences, TLC
VARIABLES x, y, s
Init == x = 0 /\ y = 0 /\ s = "n0"
Next == \/ x < 30 /\ x' = x + 1 /\ y' = y /\ s' = "n" \o ToString(x' * 1000 + y')
        \/ y < 30 /\ y' = y + 1 /\ x' = x /\ s' = "n" \o ToString(x' * 1000 + y')
        \/ x = 30 /\ y = 30 /\ x' = 0 /\ y' = 0 /\ s' = "n0"
Spec == Init /\ [][Next]_<<x, y, s>> /\ WF_<<x, y, s>>(Next)
Live == []<>(x = 0)
Bad == <>[](x > 0)
====
