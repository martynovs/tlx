---- MODULE SN22 ----
\* A function on 0..n (Map shape) in a set holding a function on 0..1 with
\* string values: when f's domain is 0..1, TLC compares the values, an
\* integer with a string, and fails.
EXTENDS Naturals
VARIABLES n, f
Init == n = 0 /\ f = <<>>
Next == /\ n < 4
        /\ n' = n + 1
        /\ f' = [i \in 0..n |-> n]
Inv == f \notin {<<>>, [i \in 0..1 |-> "s"]} \/ n = 0
====
