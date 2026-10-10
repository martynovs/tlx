---- MODULE FS2 ----
\* FoldSet with a non-set third argument: an error in both (TLC's Java
\* override is declared over Enumerable and dies with a ClassCastException;
\* tlx reports the argument error instead - see docs/compat.md D-7).
EXTENDS FiniteSetsExt
ASSUME \E z \in {FoldSet(LAMBDA p, q : p + q, 0, 3)} : TRUE
VARIABLE x
Init == x = 0
Next == x' = x
====
