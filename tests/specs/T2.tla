---- MODULE T2 ----
\* TLCGet("spec") and TLCGet("action") in breadth-first search.
EXTENDS Naturals, TLC
VARIABLE x
vars == <<x>>
Init == x = 0
A == x < 3 /\ x' = x + 1
B(n) == x = 1 /\ x' = n
Next == A \/ B(7) \/ (\E i \in {1} : x = 0 /\ x' = 5)
Spec == Init /\ [][Next]_vars /\ WF_vars(A)
Con == x < 10
ACon == x' # 6
Live == <>(x > 0)
Mono == [][x' > x]_vars
Loc(l1, c1, l2, c2) == [beginLine |-> l1, beginColumn |-> c1, endLine |-> l2, endColumn |-> c2, module |-> "T2"]
Named(n, l1, c1, l2, c2) == [name |-> n, location |-> Loc(l1, c1, l2, c2)]
Expected ==
    [ variables |-> {Named("x", 4, 10, 4, 10)},
      inits |-> {Named("Init", 6, 9, 6, 13)},
      actions |-> {Named("A", 7, 6, 7, 24), Named("Next", 9, 38, 9, 52),
                   [name |-> "B", location |-> Loc(8, 9, 8, 23), context |-> [n |-> 7], parameters |-> <<"n">>]},
      invariants |-> {Named("Inv", 29, 8, 30, 125)},
      constraints |-> {Named("Con", 11, 1, 11, 13)},
      actionconstraints |-> {Named("ACon", 12, 1, 12, 14)},
      impliedinits |-> {},
      impliedactions |-> {Named("Mono", 14, 11, 14, 23)},
      impliedtemporals |-> {Named("Live", 13, 9, 13, 17)},
      temporals |-> {Named("UnnamedAction", 10, 34, 10, 43)} ]
Inv == /\ TLCGet("spec") = Expected
       /\ TLCGet("action") = [name |-> "UnnamedAction", location |-> [Loc(0, 0, 0, 0) EXCEPT !.module = "--TLA+ BUILTINS--"]]
====
