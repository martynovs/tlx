---- MODULE ST2 ----
\* SetToSeq of a set of incomparable elements (TLC's set normalization has
\* no order across types, docs/compat.md): both checkers must report it.
EXTENDS Naturals, Sequences, TLC, SequencesExt
VARIABLE v
Init == v = SetToSeq({1, "a"})
Next == UNCHANGED v
====
