---- MODULE VL5 ----
\* VIEW with weak and strong fairness of different actions (found by
\* random testing against TLC): fairness is checked on the steps from the
\* expanded states, the property's tableau on the successor states.
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
TA == {<<0, 0, 0, 0>>, <<0, 0, 2, 0>>, <<1, 0, 0, 1>>, <<1, 0, 1, 0>>,
       <<1, 0, 1, 1>>, <<1, 1, 1, 1>>, <<2, 0, 0, 1>>, <<2, 0, 1, 0>>}
TB == {<<0, 0, 0, 0>>, <<0, 1, 1, 1>>, <<1, 0, 0, 0>>, <<1, 0, 0, 1>>,
       <<2, 0, 0, 0>>, <<2, 0, 0, 1>>, <<2, 0, 1, 1>>, <<2, 1, 0, 0>>}
A == \E t \in TA : t[1] = x /\ t[2] = y /\ x' = t[3] /\ y' = t[4]
B == \E t \in TB : t[1] = x /\ t[2] = y /\ x' = t[3] /\ y' = t[4]
Next == A \/ B
Init == x = 0 /\ y = 0
Spec == Init /\ [][Next]_vars /\ WF_vars(A) /\ SF_vars(B)
V == (x + y) % 2
Prop == ~[](x = 0)
====
