---- MODULE SubsetEagerFilterError ----
EXTENDS Naturals
VARIABLES s, t
Init == s = {} /\ t = {1, 2}
Next == UNCHANGED <<s, t>>
\* The finite filter is constructed before subset inclusion is tested,
\* even when there are no elements to test on the left.
Inv == s \subseteq {i \in t : i = 1 \/ 1 \div (i - 2) = 0}
====
