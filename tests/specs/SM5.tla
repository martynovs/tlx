---- MODULE SM5 ----
\* The Bags natives look keys up with TLC's equals (Bags.java linear
\* searches): a model value compares unequal to anything, but a string key
\* compared with an integer key is an error. The error happens in the
\* second state.
EXTENDS Naturals, Bags, TLC
CONSTANT K
VARIABLES x
Init == x = "c"
Next == x' = 3
Inv == /\ CopiesIn(K, (1 :> 3)) = 0
       /\ ((K :> 5) \sqsubseteq SetToBag({3})) = FALSE
       /\ (1 :> 1 @@ 2 :> 1) (+) (2 :> 2 @@ 3 :> 1) = <<1, 3, 1>>
       /\ BagUnion({(1 :> 1), (1 :> 2), (2 :> 1)}) = <<3, 1>>
       /\ (("c" :> 5) \ominus SetToBag({x})) # EmptyBag
====
