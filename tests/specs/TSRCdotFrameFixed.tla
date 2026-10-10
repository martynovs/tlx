---- MODULE TSRCdotFrameFixed ----
\* compat.md D-19: TLC fixes a variable that is assigned before the
\* composition (UNCHANGED S) in the intermediate state too, so A(n) /\ S' = {}
\* has no step there and TLC finds Next disabled; by the definition of
\* A \cdot B (\E t : A(s, t) /\ B(t, u)) the intermediate S is {} and the final
\* one S: Next is enabled and Prop is violated.
EXTENDS Naturals
VARIABLE x, S
Init == x = 0 /\ S = {1, 2}
A(n) == x' = n
B(n) == x' = n + 1
Next ==
    /\ x = 0
    /\ UNCHANGED S
    /\ \E n \in S : (A(n) /\ S' = {}) \cdot B(n)
Spec == Init /\ [][Next]_<<S, x>>
Prop == ~(ENABLED Next)
====
