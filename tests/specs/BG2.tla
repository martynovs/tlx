---- MODULE BG2 ----
\* (+) of a non-bag is an error with TLC's override (the TLA+ definition
\* would add the counts).
EXTENDS Bags, Integers
VARIABLE b
Init == b = [x |-> 1]
Next == b' = b (+) [x |-> 0]
====
