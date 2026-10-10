---- MODULE PredIntRange ----
\* Records built from products, quotients, remainders and GCDs of bounded
\* integers (DyadicRationals: Half, Add) are typed, so the actions and the
\* invariant compile natively.
EXTENDS Naturals, DyadicRationals
VARIABLES w, r

Init == /\ w = [i \in 1..3 |-> IF i = 1 THEN One ELSE Zero]
        /\ r = 0

Next == \E i, j \in 1..3 :
          /\ i # j
          /\ w[i] # Zero
          /\ w[i].den < 16
          /\ w' = [w EXCEPT ![i] = Half(@), ![j] = Add(@, Half(w[i]))]
          /\ r' = (r * 3 + w[i].den) % 7

Total == Add(Add(w[1], w[2]), w[3]) = One

\* Violated once some weight is down to 1/16.
Coarse == \A i \in 1..3 : w[i].den \div 2 < 8
====
