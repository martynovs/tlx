---- MODULE LVM2 ----
\* As LVM1 (a layout miss after the search), with a violated
\* property: q may change forever.
EXTENDS Naturals, Sequences
VARIABLES x, q
mq == {x, IF q = <<>> THEN x ELSE x + 10}
Init == x = 0 /\ q = <<>>
Next == \/ x < 4 /\ x' = x + 1 /\ q' = q
        \/ q = <<>> /\ q' = <<x>> /\ x' = x
        \/ q # <<>> /\ q' = <<>> /\ x' = x
Spec == Init /\ [][Next]_<<x, q>> /\ WF_<<x, q>>(Next)
Prop == [](x > 100 => 99 \notin mq) /\ <>[](mq = {x})
====
