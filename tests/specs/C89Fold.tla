---- MODULE C89Fold ----
\* The compiled tier folds constant expressions and materializes
\* definitions only where they create no strings (strings are ordered by
\* creation). `Tail`, `SubSeq` and `\o` of number sequences create
\* none (folded, materialized); of strings they do: "py" must be created
\* after "qx" (the first step), so that CHOOSE picks "qx" in Inv.
EXTENDS Naturals, Sequences, TLC
VARIABLES n, s, q
Rest == Tail(q)
Pair == SubSeq(q, 1, 2) \o <<n>>
Init == n = 0 /\ s = {} /\ q = <<3, 1, 2>>
Next == /\ n < 3
        /\ n' = n + 1
        /\ s' = IF n = 0 THEN {"q" \o "x"} ELSE s \cup {"p" \o "y"}
        /\ q' = IF n = 1 THEN Tail(<<7, 8, 9>>) \o SubSeq(<<4, 5>>, 1, 1) ELSE q
Inv == /\ n < 2 \/ (CHOOSE v \in s : TRUE) = "qx"
       /\ Len(Rest) < 3
       /\ Pair[3] = n
====
