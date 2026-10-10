---- MODULE LE2 ----
\* An action predicate of a temporal property fails on the step
\* from n = 2, in the middle of the search: liveness reports the evaluation
\* error without exploring the state space again.
EXTENDS Naturals
VARIABLES n
Init == n = 0
Next == n < 4 /\ n' = n + 1
Spec == Init /\ [][Next]_n /\ WF_n(Next)
Prop == []<><<n' = 10 \div (n - 2)>>_n
====
