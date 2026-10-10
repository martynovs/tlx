---- MODULE SN12 ----
\* Fuzzer findings: Bags.java's BagOfAll does not check the counts
\* of its bag: an image seen once keeps its count as is, whatever it is
\* (only counts that are added up must be integers). SN13: an operator
\* whose body is a literal.
EXTENDS Naturals, Bags, FiniteSets
VARIABLES n, b
Init == n = 0 /\ b = BagOfAll(LAMBDA x : x, <<"a", 3>>)
Next == /\ n < 3
        /\ n' = n + 1
        /\ b' = BagOfAll(LAMBDA x : x, [k \in 1..n + 1 |-> IF k = 1 THEN "c" ELSE k])
Inv == /\ BagIn(2, BagOfAll(LAMBDA x : x, <<n, 3>>))
       /\ BagOfAll(LAMBDA x : x, [a |-> "b"]) = [a |-> "b"]
       /\ BagOfAll(LAMBDA x : x * 0 + 1, <<2, 3>>) = <<5>>
       /\ DOMAIN b = 1..Cardinality(DOMAIN b)
====
