---- MODULE TVShape ----
\* Trace validation as in tlaplus/Examples' EWD998ChanTrace and CCF's traces:
\* an ndjson log of the nodes' events with vector clocks, put in a causal
\* order (VectorClocks!CausalOrder, TLC's Java order under -fp 0), each step
\* constrained by the log line at its level (TLCGet("level") in the
\* ACTION_CONSTRAINT and in the VIEW), a POSTCONDITION on the diameter and
\* the TraceMatched property on the queue (TLCGet("queue") in liveness).
EXTENDS TVShapeSys, Json, VectorClocks, TLC

Log == CausalOrder(ndJsonDeserialize("TVShape.ndjson"),
                   LAMBDA l : l.vc, LAMBDA l : ToString(l.node), LAMBDA vc : DOMAIN vc)

IsEvent(l) ==
    \/ l.event = "inc" /\ Inc(l.node) /\ counter'[l.node] = l.val
    \/ l.event = "send" /\ Send(l.node, l.to)
    \/ l.event = "recv" /\ Recv(l.node, l.from) /\ counter'[l.node] = l.val

TraceNextConstraint ==
    LET i == TLCGet("level") IN i <= Len(Log) /\ IsEvent(Log[i])

TraceView == <<vars, TLCGet("level")>>

TraceAccepted == TLCGet("stats").diameter - 1 = Len(Log)

TraceMatched ==
    [](TLCGet("level") <= Len(Log) => [](TLCGet("queue") \in Nat \ {0} \/ TLCGet("level") > Len(Log)))
====
