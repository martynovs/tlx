---- MODULE LiveTraceMatched ----
\* The trace-validation idiom (etcd's and CCF's TraceMatched): with a VIEW
\* that includes the log position, the queue holds one state after each
\* expansion until the log is consumed; the property only fails if the
\* search stops early. Holds here; x stops at 5 of a log of 6 entries in
\* LiveTraceMatchedFail.
EXTENDS Naturals, Sequences, TLC
CONSTANT Last
VARIABLE l, x
Log == <<0, 1, 1, 2, 3, 5>>
Init == l = 1 /\ x = Log[1]
Next == /\ l < Len(Log)
        /\ l < Last
        /\ x' \in {x, x + 1, x + 2}
        /\ x' = Log[l + 1]
        /\ l' = l + 1
Spec == Init /\ [][Next]_<<l, x>>
View == <<x, l>>
TraceMatched == [](l <= Len(Log) => [](TLCGet("queue") = 1 \/ l >= Len(Log)))
====
