---- MODULE LV3B1 ----
\* <<A>>_v atoms of properties, evaluated from one enumeration of
\* A per state in the interpreter: A covered by the next-state relation
\* (P5), A outside it that leaves a variable undetermined (P1), with more
\* steps than the bound (P3), or not enumerable (P2: y' # y before y is
\* determined), and a violated one (P4).
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Inc == x < 3 /\ x' = x + 1 /\ y' = y
Wrap == x = 3 /\ x' = 0 /\ y' = 1 - y
Init == x = 0 /\ y = 0
Next == Inc \/ Wrap
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
IncX == x' = x + 1
Big == x' \in 0..200 /\ x' # x
Flip == y' # y /\ x' = 0
P1 == []<><<IncX>>_x
P2 == []<><<Flip>>_y
P3 == []<><<Big>>_x
P4 == []<><<x' = x + 2>>_x
P5 == []<><<Wrap>>_vars
====
