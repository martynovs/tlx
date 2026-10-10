---- MODULE SA5 ----
\* A SPECIFICATION reached through a named instance: TLC sees its
\* `[][Next]_vars` under the instance's substitutions, so Next is one
\* unnamed action located at the INSTANCE definition.
EXTENDS Naturals, TLC
VARIABLES x, y
I == INSTANCE SA1I WITH N <- 3
Spec == I!Spec
Acts == TLCGet("spec").actions =
  {[name |-> "UnnamedAction", location |-> [beginLine |-> 7, beginColumn |-> 1, endLine |-> 7, endColumn |-> 30, module |-> "SA5"]]}
Inv == x + y < 4
====
