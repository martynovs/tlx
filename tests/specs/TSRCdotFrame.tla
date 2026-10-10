---- MODULE TSRCdotFrame ----
\* Action composition whose steps leave a variable undetermined: it keeps
\* its value in the intermediate state; the conjuncts around the
\* composition determine it in the final state.
EXTENDS Naturals
VARIABLE x, S
Init == x = 0 /\ S = {1, 2}
A(n) == x' = n
B(n) == x' = n + 1
Next ==
    \/ /\ x = 0
       /\ UNCHANGED S
       /\ \E n \in S : n \in S /\ A(n) \cdot B(n)
    \/ /\ x = 2
       /\ \E n \in S : A(n) \cdot B(n) /\ UNCHANGED S
Spec == Init /\ [][Next]_<<S, x>>
====
