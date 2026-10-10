---- MODULE TSRNoInit ----
\* No variables and no initial predicate: TLC finds no states.
EXTENDS Naturals
ASSUME 1 + 1 = 2
Spec == [][TRUE]_<<>>
====
