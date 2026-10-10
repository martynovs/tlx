---- MODULE CPOnceArg ----
\* By-name arguments captured into interpreter fallbacks (Interp regions):
\* an argument that fails is not evaluated unless read (TLC passes operator
\* arguments lazily), and a read argument is evaluated once per fallback.
\* Seen is a state-level definition that typed code computes (materialized)
\* and a fallback reads too.
EXTENDS Naturals, Sequences, FiniteSets, TLC
VARIABLE x, y
Init == x = 0 /\ y = <<>>
Nxt == x + 1
Seen == {n \in 0..4 : n <= x}
Next == \/ /\ x < 4
           /\ x' = Nxt
           /\ y' = Append(y, Nxt)
        \/ /\ x = 4
           /\ UNCHANGED <<x, y>>
Keep(s, bad) == Len(SelectSeq(s, LAMBDA e: e > 0 \/ Cardinality(bad) = 0))
Both(s, good) == Len(SelectSeq(s, LAMBDA e: e \in good /\ e - 1 \in good \cup {0}))
Inv == /\ Keep(y, {1 \div (x - x)}) = Len(y)
       /\ Both(y, {n \in 0..10 : n <= Nxt}) = Len(SelectSeq(y, LAMBDA e: e < Nxt))
       /\ Len(SelectSeq(y, LAMBDA e: e < Nxt)) = Len(y)
       /\ Cardinality(Seen) = Nxt
       /\ SortSeq(<<Cardinality(Seen), x>>, LAMBDA a, b: a < b) = <<x, x + 1>>
====
