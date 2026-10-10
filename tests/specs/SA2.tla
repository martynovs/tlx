---- MODULE SA2 ----
\* Action splitting as in TLC under a named INSTANCE: its definitions are
\* actions named `I!Op` (with their constant arguments), located at the
\* INSTANCE definition; a 0-ary constant definition is an action named after
\* it; LET and existentials over constant sets are split.
EXTENDS Naturals, TLC
VARIABLES x, y
I == INSTANCE SA1I WITH N <- 3
Never == FALSE
Op(A, k) == A /\ k > 0
Next == \/ \E k \in {1, 2} : I!B(k)
        \/ I!A
        \/ Never
        \/ \E k \in {} : x' = k /\ y' = y
        \/ Op(x' = x + 7 /\ y' = y, 1) /\ x = 2
        \/ LET d == 2 IN \E k \in {d} : x = 3 /\ x' = x + k /\ y' = y
Spec == I!Init /\ [][Next]_<<x, y>>
Loc(l, c1, l2, c2) == [beginLine |-> l, beginColumn |-> c1, endLine |-> l2, endColumn |-> c2, module |-> "SA2"]
Acts == TLCGet("spec").actions =
  {[name |-> "I!B", location |-> Loc(8, 1, 8, 30), context |-> [k |-> 1], parameters |-> <<"k">>],
   [name |-> "I!B", location |-> Loc(8, 1, 8, 30), context |-> [k |-> 2], parameters |-> <<"k">>],
   [name |-> "I!A", location |-> Loc(8, 1, 8, 30)],
   [name |-> "Never", location |-> Loc(13, 12, 13, 16)],
   [name |-> "Next", location |-> Loc(14, 12, 14, 41)],
   [name |-> "Next", location |-> Loc(15, 12, 15, 47)],
   [name |-> "Next", location |-> Loc(16, 41, 16, 69)]}
Inv == y < 2
====
