---- MODULE ReprEmptyFcn ----
\* The empty function: `<<>>`, `[k \in {} |-> 0]` and the empty bag are one
\* value (equal, one state), distinct from the empty set; DOMAIN, Len,
\* Append, `@@` and EXCEPT of an absent integer key agree on all of them.
EXTENDS Naturals, Sequences, TLC
VARIABLES y, pc
Init == y = <<>> /\ pc = 0
Next == \/ pc = 0 /\ y' = [k \in {} |-> 0] /\ pc' = 1
        \/ pc = 0 /\ y' = [k \in {1} |-> 0] /\ pc' = 9
        \/ pc = 9 /\ y' = Tail(y) /\ pc' = 1
        \/ pc = 1 /\ y' = [y EXCEPT ![1] = 5] /\ pc' = 2
        \/ pc = 1 /\ y' = Append(y, 7) /\ pc' = 3
        \/ pc = 1 /\ y' = y @@ (4 :> 1) /\ pc' = 4
        \/ pc = 2 /\ y' = y @@ <<8>> /\ pc' = 5
        \/ pc \in {3, 4, 5} /\ y' = [k \in DOMAIN y |-> y[k] + Len(SelectSeq(<<1>>, LAMBDA e : e > 0))] /\ pc' = 6
Inv == /\ pc \in {1, 2} => y = <<>> /\ DOMAIN y = {} /\ y # <<1>>
       /\ pc = 5 => y = <<8>>
====
