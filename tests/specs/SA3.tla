---- MODULE SA3 ----
\* A SPECIFICATION from an instance: TLC does not split its next-state
\* relation (one unnamed action, located at the INSTANCE statement).
EXTENDS Naturals, TLC
VARIABLES x, y
INSTANCE SA1I WITH N <- 3
Acts == TLCGet("spec").actions =
  {[name |-> "UnnamedAction", location |-> [beginLine |-> 6, beginColumn |-> 1, endLine |-> 6, endColumn |-> 25, module |-> "SA3"]]}
Inv == x + y < 4
====
