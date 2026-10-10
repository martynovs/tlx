---- MODULE LiveQueue ----
\* TLCGet("queue") in a temporal property: TLC evaluates the liveness
\* predicates once a state's successors are generated and queued (initial
\* states once queued), and reports this violation during its search.
EXTENDS Naturals, TLC
VARIABLE x, y
Init == x = 0 /\ y = 0
Next == /\ x < 4
        /\ \/ x' = x + 1 /\ y' = y
           \/ x' = x + 2 /\ y' = y
           \/ x' = x + 1 /\ y' = 1 - y
Spec == Init /\ [][Next]_<<x, y>>
View == x
Prop == [](x <= 4 => [](TLCGet("queue") <= 1 \/ x > 4))
====
