---- MODULE CausalOrderTLC ----
\* VectorClocks!CausalOrder as TLC's Java override computes it: a pass over
\* the nodes takes each node's first line without unseen predecessors, the
\* nodes in the order of TLC's HashMap of their ids under -fp 0 (with other
\* -fp values TLC orders concurrent lines differently: docs/compat.md D-25).
EXTENDS VectorClocks, Sequences, Naturals, TLC
L(n) == [node |-> n, vc |-> (n :> 1)]
Nodes(o) == [i \in 1..Len(o) |-> o[i].node]
O(log) == Nodes(CausalOrder(log, LAMBDA l : l.vc, LAMBDA l : l.node, LAMBDA vc : DOMAIN vc))
ASSUME O(<<L(3), L(1), L(2), L(7)>>) = <<1, 2, 7, 3>>
ASSUME O(<<L("c"), L("a"), L("b"), L("d")>>) = <<"d", "a", "c", "b">>
ASSUME O(<<L(0), L(1), L(2), L(3), L(4)>>) = <<1, 2, 3, 4, 0>>
M(n, vc) == [node |-> n, vc |-> vc]
\* c's second line saw b's first; b's second saw a's second.
Log1 == << M("c", [c |-> 2, b |-> 1]), M("a", [a |-> 1]), M("b", [b |-> 1]), M("c", [c |-> 1]),
           M("a", [a |-> 2]), M("d", [d |-> 1]), M("b", [b |-> 2, a |-> 2]), M("d", [d |-> 2]) >>
O1 == CausalOrder(Log1, LAMBDA l : l.vc, LAMBDA l : l.node, LAMBDA vc : DOMAIN vc)
ASSUME [i \in 1..Len(O1) |-> <<O1[i].node, O1[i].vc[O1[i].node]>>]
       = <<<<"d", 1>>, <<"a", 1>>, <<"c", 1>>, <<"b", 1>>, <<"d", 2>>, <<"a", 2>>, <<"c", 2>>, <<"b", 2>>>>
Log2 == << M(3, 3 :> 1), M(1, 1 :> 1), M(2, 2 :> 1 @@ 3 :> 1), M(1, 1 :> 2), M(3, 3 :> 2), M(7, 7 :> 1) >>
O2 == CausalOrder(Log2, LAMBDA l : l.vc, LAMBDA l : l.node, LAMBDA vc : DOMAIN vc)
ASSUME [i \in 1..Len(O2) |-> <<O2[i].node, O2[i].vc[O2[i].node]>>]
       = <<<<1, 1>>, <<7, 1>>, <<3, 1>>, <<1, 2>>, <<2, 1>>, <<3, 2>>>>
VARIABLE x
Init == x = 0
Next == x < 2 /\ x' = x + 1
====
