---- MODULE SA4 ----
\* Actions not from a definition (TLC's unnamed actions), an operator
\* applied to an action (named after the operator) and existentials over an
\* empty set (actions without successors).
EXTENDS Naturals, TLC
VARIABLE x
Twice(A, B) == A /\ B
Step(k) == x' = x + k
Spec == /\ x = 0
        /\ [][\/ x < 3 /\ x' = x + 1
              \/ Twice(x >= 3, x' = x + 2)
              \/ \E k \in {} : x' = k
              \/ \E k \in {1} : x = 5 /\ Step(k)
              \/ \E k \in {k \in 1..3 : k > 5} : Step(k)]_x
Loc(l, c1, l2, c2) == [beginLine |-> l, beginColumn |-> c1, endLine |-> l2, endColumn |-> c2, module |-> "SA4"]
Acts == TLCGet("spec").actions =
  {[name |-> "UnnamedAction", location |-> Loc(10, 18, 10, 36)],
   [name |-> "Twice", location |-> Loc(11, 18, 11, 42)],
   [name |-> "UnnamedAction", location |-> Loc(12, 18, 12, 37)],
   [name |-> "UnnamedAction", location |-> Loc(13, 33, 13, 48)],
   [name |-> "UnnamedAction", location |-> Loc(14, 18, 14, 56)]}
Inv == x < 6
====
