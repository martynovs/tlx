---- MODULE SN13 ----
\* Fuzzer findings: TLC passes an operator whose body is a
\* numeral or string literal (LAMBDA x : 2) to a Java override as the
\* literal's value, so BagOfAll (like the folds and SortSeq) rejects it,
\* even for an empty bag; an operator with any other body (here -2) is an
\* operator. The error is raised in the second state.
EXTENDS Integers, Bags
VARIABLES n, b
Init == n = 0 /\ b = BagOfAll(LAMBDA x : -2, SetToBag({1}))
Next == /\ n < 3
        /\ n' = n + 1
        /\ b' = IF n = 0 THEN BagOfAll(LAMBDA x : x + 1, b) ELSE BagOfAll(LAMBDA x : 2, EmptyBag)
====
