---- MODULE ST1 ----
\* SequencesExt!SetToSeq / SetToSeqs (Java overrides in
\* tlc2/overrides/SequencesExt.java; the TLA+ bodies enumerate a whole
\* function set) and TLC!SetToSortSeq, which is `SortSeq(SetToSeq(S), op)`.
\* The counterexample compares the values: element order (TLC's normalized
\* order), the empty set (`<<>>` / `{<<>>}`), model values (their declaration
\* order in the config) and records / sequences of sequences.
EXTENDS Naturals, Sequences, FiniteSets, TLC, SequencesExt
CONSTANTS A, B, C
VARIABLE v
Ints == {3, 1, 2}
Strs == {"b", "a", "c"}
Recs == {[a |-> 2, b |-> "y"], [a |-> 1, b |-> "x"]}
MVs == {C, A, B}
Nested == {<<2, 1>>, <<1, 2>>}
Init == v = << SetToSeq(Ints),
             SetToSeq(Strs),
             SetToSeq(Recs),
             SetToSeq(MVs),
             SetToSeq(Nested),
             SetToSeq({}),
             SetToSeq(1..5),
             SetToSortSeq({1, 3, 2}, >),
             SetToSortSeq({3, 1, 2}, <),
             SetToSeqs({1, 2}),
             SetToSeqs({5}),
             SetToSeqs({}) >>
Next == UNCHANGED v
Inv == v = <<>>
====
