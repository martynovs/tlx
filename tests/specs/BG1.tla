---- MODULE BG1 ----
\* Bags and BagsExt with TLC's Java overrides (Bags.java, BagsExt.java),
\* which differ from the TLA+ definitions on non-bags.
EXTENDS Bags, BagsExt, Integers, Sequences, TLC
VARIABLE b
b1 == [a |-> 2, b |-> 1]
b2 == [b |-> 3, c |-> 1]
Cat(s, e) == s \o ToString(e)
ASSUME IsABag(b1) /\ ~IsABag([a |-> 0]) /\ IsABag(<<1, 2>>) /\ ~IsABag(3)
ASSUME b1 (+) b2 = [a |-> 2, b |-> 4, c |-> 1]
ASSUME b1 (-) b2 = [a |-> 2] /\ b2 (-) b1 = [b |-> 2, c |-> 1]
ASSUME BagCardinality(b1) = 3 /\ BagCardinality([i \in 1..25 |-> i]) = 325
ASSUME BagIn("a", b1) /\ ~BagIn("z", b1) /\ CopiesIn("a", b1) = 2 /\ CopiesIn("z", b1) = 0
\* CopiesIn returns the count as is (the TLA+ definition too).
ASSUME CopiesIn("a", [a |-> -1]) = -1
ASSUME BagToSet(b1) = {"a", "b"} /\ SetToBag({3, 1}) = (1 :> 1 @@ 3 :> 1) /\ EmptyBag = <<>>
ASSUME BagUnion({b1, b2}) = [a |-> 2, b |-> 4, c |-> 1] /\ BagUnion({b1, b1}) = b1 /\ BagUnion({}) = <<>>
ASSUME ~(b1 \sqsubseteq b2) /\ [a |-> 1] \sqsubseteq b1
ASSUME BagOfAll(LAMBDA e : e % 3, [i \in 1..7 |-> i]) = (0 :> 9 @@ 1 :> 12 @@ 2 :> 7)
ASSUME SubBag([a |-> 2, b |-> 1]) =
         {<<>>, [a |-> 1], [a |-> 2], [b |-> 1], [a |-> 1, b |-> 1], [a |-> 2, b |-> 1]}
ASSUME BagAdd(b1, "q") = [a |-> 2, b |-> 1, q |-> 1] /\ BagRemove(b1, "a") = [a |-> 1, b |-> 1]
\* FoldBag applies op(acc, e) once per copy, in domain order.
ASSUME SumBag([i \in 1..4 |-> i]) = 30 /\ FoldBag(Cat, "", [i \in 1..3 |-> 4 - i]) = "111223"
Init == b = EmptyBag
Next == \E e \in {"x", "y"} :
          /\ BagCardinality(b) < 4
          /\ b' = IF CopiesIn(e, b) > 1 THEN b (-) SetToBag({e}) ELSE b (+) SetToBag({e})
====
