---- MODULE SA1 ----
\* Action splitting as in TLC (Tool.getActions): a definition of an INSTANCE
\* with substitutions is one action located at the INSTANCE statement; an
\* operator applied to an action is an action named after the operator; an
\* existential over an empty set is an action without successors.
EXTENDS Naturals, TLC
VARIABLES x, y
INSTANCE SA1I WITH N <- 2
Wrap(Act) == Act /\ x' >= x
MySpec == Init /\ [][Next \/ Wrap(x' = x /\ y' = y + 5 /\ y = 1) \/ \E k \in {} : x' = k /\ y' = y]_<<x, y>>
Loc(l, c1, l2, c2) == [beginLine |-> l, beginColumn |-> c1, endLine |-> l2, endColumn |-> c2, module |-> "SA1"]
Acts == TLCGet("spec").actions =
  {[name |-> "Next", location |-> Loc(8, 1, 8, 25)],
   [name |-> "Wrap", location |-> Loc(10, 30, 10, 64)],
   [name |-> "UnnamedAction", location |-> Loc(10, 69, 10, 98)]}
Inv == y < 6
====
