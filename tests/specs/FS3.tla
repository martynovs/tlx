---- MODULE FS3 ----
\* The folds that check their argument in Java: FoldFunction / FoldSeq
\* (which delegates to it) want a function, FoldLeft / FoldRight (and the
\* ...Domain variants) a sequence.
EXTENDS Functions, SequencesExt
ASSUME \E z \in {FoldFunction(LAMBDA p, q : p + q, 0, {1, 2})} : TRUE
VARIABLE x
Init == x = 0
Next == x' = x
====
