---- MODULE TSRPropForallInv ----
\* As TSRPropForall, with []P: an invariant named by the body's location.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 4
Spec == Init /\ [][Next]_x
InvP == \A k \in {1, 2} : [](x < 2 + k)
====
