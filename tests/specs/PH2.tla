---- MODULE PH2 ----
\* SequencesExt (SequencesExt.java): AllSubSeqs, SelectInSubSeq,
\* SelectLastInSubSeq. The TLA+ definitions fold over `SUBSET DOMAIN s`,
\* whose CHOOSE order can reverse the elements; TLC's Java overrides keep
\* each subsequence in index order. Select* return the index in the
\* *original* sequence (0 when nothing matches).
EXTENDS SequencesExt, TLC, Sequences, FiniteSets, Naturals, Integers
\* Every subsequence, elements in index order (2^4 of them).
ASSUME Print("allsub4:", AllSubSeqs(<<1, 2, 3, 4>>)) =
       {<<>>, <<1>>, <<2>>, <<3>>, <<4>>,
        <<1, 2>>, <<1, 3>>, <<1, 4>>, <<2, 3>>, <<2, 4>>, <<3, 4>>,
        <<1, 2, 3>>, <<1, 2, 4>>, <<1, 3, 4>>, <<2, 3, 4>>, <<1, 2, 3, 4>>}
ASSUME Print("allsub2:", AllSubSeqs(<<1, 2>>)) = {<<>>, <<1>>, <<2>>, <<1, 2>>}
ASSUME Print("allsub0:", AllSubSeqs(<<>>)) = {<<>>}
\* SelectInSubSeq: the first index in [from, to] whose element satisfies P.
ASSUME Print("insub-hit:", SelectInSubSeq(<<1, 2, 3, 4>>, 2, 3, LAMBDA y : y > 2)) = 3
ASSUME Print("insub-none:", SelectInSubSeq(<<1, 2, 3, 4>>, 1, 4, LAMBDA y : y > 9)) = 0
\* from > to is an empty range, like TLC (not an error).
ASSUME Print("insub-empty:", SelectInSubSeq(<<1, 2, 3, 4>>, 3, 2, LAMBDA y : y > 2)) = 0
ASSUME Print("insub-last:", SelectInSubSeq(<<1, 2, 3, 4>>, 1, 4, LAMBDA y : y = 4)) = 4
\* SelectLastInSubSeq: the last index in [from, to] satisfying P.
ASSUME Print("last-hit:", SelectLastInSubSeq(<<1, 2, 3, 4>>, 2, 4, LAMBDA y : y < 4)) = 3
ASSUME Print("last-none:", SelectLastInSubSeq(<<1, 2, 3, 4>>, 1, 4, LAMBDA y : y > 9)) = 0
ASSUME Print("last-empty:", SelectLastInSubSeq(<<1, 2, 3, 4>>, 4, 2, LAMBDA y : y > 2)) = 0
ASSUME Print("last-first:", SelectLastInSubSeq(<<1, 2, 3, 4>>, 1, 3, LAMBDA y : y = 1)) = 1
\* SelectInSeq is the module's own TLA+ definition over SelectInSubSeq.
ASSUME Print("inseq:", SelectInSeq(<<1, 2, 3, 4>>, LAMBDA y : y = 3)) = 3
VARIABLE x
Init == x = 0
Next == x' = x
Spec == Init /\ [][Next]_x
====
