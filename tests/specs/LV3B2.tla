---- MODULE LV3B2 ----
\* <<A>>_v atoms of properties evaluated from one enumeration of
\* A per state (interpreter), both ways: P* hold only if their atom is true
\* on some step of the cycle, N* (~[]<>) only if it is never true there.
\* A in the next-state relation (P5, N5), outside it leaving a variable
\* undetermined (P1, N4), with more steps than the bound (P3, N2), not
\* enumerable (P2, N6: y' # y before y' is determined).
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Inc == x < 3 /\ x' = x + 1 /\ y' = y
Wrap == x = 3 /\ x' = 0 /\ y' = 1 - y
Never == x = 9 /\ x' = 0 /\ y' = y
Init == x = 0 /\ y = 0
Next == Inc \/ Wrap \/ Never
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
P1 == []<><<y' = y + 1>>_y
P2 == []<><<y' # y /\ x' = 0>>_y
P3 == []<><<x' \in 0..200 /\ x' = x + 1>>_x
P5 == []<><<Wrap>>_vars
N1 == ~[]<><<y' = y + 2>>_y
N2 == ~[]<><<x' \in 0..200 /\ x' > x + 1>>_x
N3 == ~[]<><<x' = x + 1 /\ y' = 5>>_x
N4 == ~[]<><<x' = 5>>_x
N5 == ~[]<><<Never>>_vars
N6 == ~[]<><<y' # y /\ x' = 2>>_y
====
