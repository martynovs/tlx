---- MODULE PC1 ----
\* Type-invariant bounds size the compiled layout: msgs is a set of
\* records of several kinds. Its shape seen in the pre-run (string fields
\* over all strings of the spec, integer fields with headroom) has too many
\* possible elements for a bitset; the invariant's SUBSET of a constant
\* record set makes it a bitset over exactly the possible messages.
EXTENDS Naturals, FiniteSets
CONSTANTS P
VARIABLES st, msgs, n

Msg == [type : {"req"}, from : P, to : P, k : 0..19, tag : {"lo", "hi"}]
         \cup [type : {"ack"}, to : P]
         \cup [type : {"done"}]

TypeOK == /\ st \in [P -> {"idle", "wait", "busy"}]
          /\ msgs \subseteq Msg
          /\ n \in 0..9

Init == /\ st = [p \in P |-> "idle"]
        /\ msgs = {}
        /\ n = 0

\* At most one request per process: a new one replaces the old one.
Req(p, q) == /\ st[p] = "idle"
             /\ st' = [st EXCEPT ![p] = "wait"]
             /\ msgs' = {m \in msgs : m.type /= "req" \/ m.from /= p}
                          \cup {[type |-> "req", from |-> p, to |-> q, k |-> n,
                                 tag |-> IF n % 2 = 0 THEN "lo" ELSE "hi"]}
             /\ UNCHANGED n

Ack(p) == \E m \in msgs :
            /\ m.type = "req"
            /\ m.from = p
            /\ st[p] = "wait"
            /\ msgs' = msgs \cup {[type |-> "ack", to |-> p]}
            /\ st' = [st EXCEPT ![p] = "busy"]
            /\ n' = (n + m.k + 1) % 10

Reset(p) == /\ st[p] = "busy"
            /\ st' = [st EXCEPT ![p] = "idle"]
            /\ UNCHANGED <<msgs, n>>

Done == /\ Cardinality({m \in msgs : m.type = "ack"}) = 2
        /\ [type |-> "done"] \notin msgs
        /\ msgs' = msgs \cup {[type |-> "done"]}
        /\ UNCHANGED <<st, n>>

\* Set maps and filters into the bitset of messages.
AckIdle == /\ n = 6
           /\ msgs' = msgs \cup {[type |-> "ack", to |-> q] : q \in {p \in P : st[p] = "idle"}}
           /\ UNCHANGED <<st, n>>

Forget == /\ n \in {5, 8}
          /\ msgs' = {m \in msgs : m.type /= "req" \/ m.tag = "hi"}
          /\ UNCHANGED <<st, n>>

Next == \/ \E p, q \in P : Req(p, q)
        \/ \E p \in P : Ack(p) \/ Reset(p)
        \/ Done
        \/ AckIdle
        \/ Forget

Spec == Init /\ [][Next]_<<st, msgs, n>>
====
