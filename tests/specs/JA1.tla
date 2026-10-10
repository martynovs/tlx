---- MODULE JA1 ----
\* Loops over a bitset of messages of several record types (the JIT
\* specializes them per union arm and masks the ranks of a tested type).
EXTENDS Integers, FiniteSets
P == {1, 2}
B == 0..2
Msg == [type : {"req"}, from : P, bal : B]
       \cup [type : {"ack"}, from : P, bal : B, val : 0..1]
       \cup [type : {"done"}]
VARIABLES msgs, decided, cnt
Init == msgs = {} /\ decided = -1 /\ cnt = 0
Req(p, b) == /\ b > 0
             /\ ~\E m \in msgs : m.type = "req" /\ m.from = p /\ m.bal >= b
             /\ msgs' = msgs \cup {[type |-> "req", from |-> p, bal |-> b]}
             /\ UNCHANGED <<decided, cnt>>
Ack(p) == /\ \E m \in msgs :
                /\ m.type = "req"
                /\ m.from # p
                /\ \E v \in 0..1 : msgs' = msgs \cup {[type |-> "ack", from |-> p, bal |-> m.bal, val |-> v]}
          /\ cnt' = Cardinality({x \in msgs : x.type = "ack"})
          /\ UNCHANGED decided
Decide == /\ decided = -1
          /\ \E b \in B :
                LET A == {m \in msgs : m.type = "ack" /\ m.bal = b}
                IN /\ A # {}
                   /\ \A m \in A : m.val = (CHOOSE x \in A : TRUE).val
                   /\ decided' = (CHOOSE x \in A : TRUE).val
          /\ msgs' = msgs \cup {[type |-> "done"]}
          /\ UNCHANGED cnt
Next == \/ \E p \in P, b \in B : Req(p, b)
        \/ \E p \in P : Ack(p)
        \/ Decide
TypeOK == /\ msgs \subseteq Msg
          /\ \A m \in msgs : m.type = "ack" => m.val \in 0..1
Inv == decided # -1 => \E m \in msgs : m.type = "done"
Bals == {m.bal : m \in {x \in msgs : x.type # "done"}}
Inv2 == /\ Cardinality(Bals) <= 3
        /\ \A m \in msgs : m.type = "done" \/ m.from \in P
        /\ cnt <= Cardinality({m \in msgs : m.type = "ack" /\ m.val \in {0, 1}})
====
