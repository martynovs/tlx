---- MODULE TVShapeSys ----
\* (The system specified by TVShape: nodes count, send their count to each
\* other and take the maximum on receipt, in any order.)
EXTENDS Naturals, Sequences
VARIABLES counter, inbox, mood
vars == <<counter, inbox, mood>>
Node == 0..2
Max(a, b) == IF a > b THEN a ELSE b
Init == counter = [n \in Node |-> 0] /\ inbox = [n \in Node |-> <<>>] /\ mood = 0
\* (mood is not logged: the trace validation branches on it.)
Inc(n) == counter' = [counter EXCEPT ![n] = @ + 1] /\ mood' \in {0, 1} /\ UNCHANGED inbox
Send(n, m) ==
    /\ n # m
    /\ inbox' = [inbox EXCEPT ![m] = Append(@, [from |-> n, val |-> counter[n]])]
    /\ UNCHANGED <<counter, mood>>
Recv(n, from) ==
    \E i \in DOMAIN inbox[n] :
        /\ inbox[n][i].from = from
        /\ counter' = [counter EXCEPT ![n] = Max(@, inbox[n][i].val)]
        /\ inbox' = [inbox EXCEPT ![n] = SubSeq(@, 1, i - 1) \o SubSeq(@, i + 1, Len(@))]
        /\ UNCHANGED mood
Next == \E n \in Node : \/ Inc(n)
                        \/ \E m \in Node : Send(n, m) \/ Recv(n, m)
Spec == Init /\ [][Next]_vars
====
