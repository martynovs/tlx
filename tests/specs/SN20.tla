---- MODULE SN20 ----
\* Compiled comparisons of function-like values with different domains
\* (MCLeastCircularSubstring): functions on 0..n (a Map shape) compared with
\* tuples and other functions. TLC compares domains of different sizes as
\* unequal and, for one size, compares the integer domain elements before
\* the values, so none of these is an error (f # h: domains 0..1 and 1..2
\* differ, the string values are never compared).
EXTENDS Naturals
VARIABLES n, f, h
Init == n = 0 /\ f = <<>> /\ h = <<"a", "b">>
Next == /\ n < 4
        /\ n' = n + 1
        /\ f' = [i \in 0..n |-> n]
        /\ h' = h
Corpus == {<<>>} \cup {[i \in 0..k |-> k] : k \in 0..2}
Inv == /\ (f \in Corpus) = (n < 4)
       /\ f \notin {<<1, 2>>, <<3, 3, 3>>, [i \in 2..3 |-> 1]}
       /\ f # h
       /\ (f = <<>>) = (n = 0)
====
