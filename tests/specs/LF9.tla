---- MODULE LF9 ----
\* Constant subformulas of a property are folded to TRUE / FALSE
\* before TLC counts the branches (Liveness.astToLive): 1 branch here.
EXTENDS Naturals
CONSTANT N
VARIABLES x
Init == x = 0
Next == x' = (x + 1) % N
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Prop == (N > 5 => <>[](x = 0)) /\ (N > 1 => []<>(x = 1))
====
