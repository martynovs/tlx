---- MODULE CMP75Rec ----
\* Membership of a constant outside the element shape of an encoded set
\* (like byihive's VTPRcvPrepared) Rcv(h, i) is instantiated for
\* h, i \in H \cup I, so it tests records such as [type |-> "P", vi |-> h1]
\* that msgs never holds; such a membership is FALSE, while a record whose
\* field set differs and a value of another kind are compared as TLC does.
EXTENDS Naturals, FiniteSets

CONSTANTS H, I

VARIABLES msgs, got

Init == msgs = {} /\ got = {}

SendH(h) == msgs' = msgs \cup {[type |-> "P", vh |-> h]} /\ UNCHANGED got
SendI(i) == msgs' = msgs \cup {[type |-> "P", vi |-> i]} /\ UNCHANGED got

Rcv(h, i) ==
  /\ [type |-> "P", vh |-> h] \in msgs
  /\ [type |-> "P", vi |-> i] \in msgs
  /\ [type |-> "Q"] \notin msgs
  /\ got' = got \cup {h, i}
  /\ UNCHANGED msgs

Next == \/ \E h \in H : SendH(h)
        \/ \E i \in I : SendI(i)
        \/ \E h, i \in H \cup I : Rcv(h, i)

Inv == got \subseteq H \cup I /\ Cardinality(got) <= 4
====
