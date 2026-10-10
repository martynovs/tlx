---- MODULE FS1 ----
\* Folds of the community modules, evaluated natively as TLC does
\* (FiniteSetsExt.java foldSet, Functions.java foldFunction /
\* foldFunctionOnSet, SequencesExt.java foldLeft / foldRight / ...Domain):
\* iterate the (normalized, sorted) elements and combine `op(x, acc)`
\* (`op(acc, x)` for the *Left variants), so non-commutative operators and
\* intermediate overflow follow TLC's order rather than the TLA+ definition.
EXTENDS FiniteSetsExt, Functions, SequencesExt, TLC, Integers, Sequences, Naturals
\* `<<<<a, b>, c>, d>>` is not valid TLA+ (SANY and TLC reject it, and so
\* does tlx -- FE-8); build nested pairs through Nest instead.
Nest(a, b) == <<a, b>>
\* The sorted order is -3, 5 with acc = op(x, acc): 5 - (-3 - 2) = 10.
ASSUME Print("noncomm:", FoldSet(LAMBDA p, q : p - q, 2, {-3, 5})) = 10
\* Fuzzer finding #3: the domain of a function is sorted, -3 then 5.
ASSUME Print("fuzz3:", FoldSet(LAMBDA p, q : p - q, 2, DOMAIN ((5 :> 5) @@ ((-3) :> 2147483646)))) = 10
\* Fuzzer finding #13: the duplicate -3 collapses, so the set is {-3, 2147483647}
\* and the fold is 2147483647 + (-3 + 1) = 2147483645 (the TLA+ definition
\* folds in another order and overflows).
ASSUME Print("fuzz13:", FoldSet(LAMBDA p, q : p + q, 1, {(-3), (-3), 2147483647})) = 2147483645
ASSUME Print("empty:", FoldSet(LAMBDA p, q : <<p, q>>, 7, {})) = 7
ASSUME Print("strings:", FoldSet(LAMBDA p, q : p \o q, "Z", {"a", "bb", "c"})) = "cbbaZ"
ASSUME Print("records:", FoldSet(LAMBDA p, q : [f |-> p.f + q.f], [f |-> 100], {[f |-> 1], [f |-> 2], [f |-> 3]})) = [f |-> 106]
ASSUME Print("pred:", FoldSet(LAMBDA p, q : p \o q, "", {x \in {"c", "a", "b"} : x # "b"})) = "ca"
ASSUME Print("interval:", FoldSet(LAMBDA p, q : p - q, 100, 1..4)) = 102
ASSUME Print("big:", FoldSet(LAMBDA p, q : <<p, q>>, 0, 1000000..1000003)) = Nest(1000003, Nest(1000002, Nest(1000001, Nest(1000000, 0))))
ASSUME Print("subset:", FoldSet(LAMBDA p, q : <<p, q>>, 0, SUBSET {1, 2})) = Nest({1, 2}, Nest({2}, Nest({1}, Nest({}, 0))))
ASSUME Print("union:", FoldSet(LAMBDA p, q : <<p, q>>, 0, UNION {{{1, 2}, {3}}, {{4}}})) = Nest({1, 2}, Nest({4}, Nest({3}, 0)))
\* FoldFunction folds over DOMAIN f (an interval here), sorted.
ASSUME Print("foldfun:", FoldFunction(LAMBDA p, q : p - q, 10, [i \in 1..3 |-> i])) = -8
ASSUME Print("foldfunonset:", FoldFunctionOnSet(LAMBDA p, q : p - q, 10, [i \in 1..3 |-> i*10], {3, 1})) = 30
ASSUME Print("foldfuninterval:", FoldFunction(LAMBDA p, q : p - q, 10, [i \in 2..4 |-> i])) = -7
\* FoldSeq delegates to FoldFunction, so a sequence folds over 1..Len; a
\* record is accepted too (its domain is a set of strings).
ASSUME Print("foldseq:", FoldSeq(LAMBDA p, q : p - q, 10, <<1, 2, 3>>)) = -8
ASSUME Print("foldseqrec:", FoldSeq(LAMBDA p, q : p - q, 10, [a |-> 1, b |-> 2])) = 11
ASSUME Print("foldleft:", FoldLeft(LAMBDA p, q : <<p, q>>, 0, <<1, 2, 3>>)) = Nest(Nest(Nest(0, 1), 2), 3)
ASSUME Print("foldright:", FoldRight(LAMBDA p, q : <<p, q>>, <<1, 2, 3>>, 0)) = Nest(1, Nest(2, Nest(3, 0)))
ASSUME Print("foldleftdomain:", FoldLeftDomain(LAMBDA p, q : <<p, q>>, 0, <<"a", "b">>)) = Nest(Nest(0, 1), 2)
ASSUME Print("foldrightdomain:", FoldRightDomain(LAMBDA p, q : <<p, q>>, <<"a", "b">>, 0)) = Nest(1, Nest(2, 0))
\* FoldLeft takes anything with a 1..n domain (Value.toTuple in Java).
ASSUME Print("foldleft-setdom:", FoldLeft(LAMBDA p, q : <<p, q>>, 0, [i \in {1, 2, 3} |-> i * 10])) = Nest(Nest(Nest(0, 10), 20), 30)
ASSUME Print("foldleft-empty:", FoldLeft(LAMBDA p, q : p - q, 99, <<>>)) = 99
ASSUME Print("foldright-empty:", FoldRight(LAMBDA p, q : p - q, <<>>, 99)) = 99
ASSUME Print("foldld-empty:", FoldLeftDomain(LAMBDA p, q : p - q, 99, <<>>)) = 99
ASSUME Print("foldrd-empty:", FoldRightDomain(LAMBDA p, q : p - q, <<>>, 99)) = 99
VARIABLE x
Init == x = 0
Next == x' = x + 1 /\ x < 2
Spec == Init /\ [][Next]_x
====
