---- MODULE CMP75Sub ----
\* A loop-invariant SUBSET source inside a binding construct of a
\* compiled predicate (like bcastByz): the fairness action
\* \E p \in Corr : \E new \in SUBSET sent : ... is evaluated on every
\* edge; R9 hoisting took the iteration source SUBSET sent for a boxed
\* region, which the host cannot evaluate as a value.
EXTENDS Naturals, FiniteSets

CONSTANT N

VARIABLES sent, rcvd, Corr

Proc == 1 .. N

Init == /\ sent = {}
        /\ rcvd = [p \in Proc |-> {}]
        /\ Corr \in {Proc \ {q} : q \in Proc}

Send(p) == /\ p \notin sent
           /\ sent' = sent \cup {p}
           /\ UNCHANGED <<rcvd, Corr>>

Receive(p) == /\ \E new \in SUBSET sent :
                   /\ new # {}
                   /\ rcvd' = [rcvd EXCEPT ![p] = rcvd[p] \cup new]
              /\ UNCHANGED <<sent, Corr>>

Next == \E p \in Corr : Send(p) \/ Receive(p)

vars == <<sent, rcvd, Corr>>

Spec == Init /\ [][Next]_vars /\ WF_vars(\E p \in Corr : Receive(p))

AllReceived == <>(\A p \in Corr : rcvd[p] = sent)

TypeOK == rcvd \in [Proc -> SUBSET Proc]
====
