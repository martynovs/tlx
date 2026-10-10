---- MODULE TSRCdotChained ----
\* Chained action composition: TLC counts the intermediate states of the
\* outermost composition only (A \cdot B \cdot C is (A \cdot B) \cdot C:
\* the 4 states of A \cdot B, not the 2 of A).
EXTENDS Naturals
VARIABLE x
Init == x = 0
A(S) == \E n \in S : x' = x + n
Next == x = 0 /\ A({1, 2}) \cdot A({3, 4}) \cdot A({5})
Spec == Init /\ [][Next]_x
====
