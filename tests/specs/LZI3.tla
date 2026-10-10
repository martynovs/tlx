---- MODULE LZI3 ----
\* The initial states of x \in {r \in S : P} come in the enumeration
\* order of S (here S1 \cup S2: S1 first, not normalized), and the BFS order
\* follows: the invariant fails first in a successor of [a |-> 9, b |-> 37],
\* after the successors of the initial states with a = 9 and b < 37 only.
EXTENDS Naturals
VARIABLE x
S == [a : {9, 8}, b : 1..40] \cup [a : {1, 2}, b : 1..40]
Init == x \in {r \in S : r.b % 2 = 1}
Next == x.b < 60 /\ x' = [x EXCEPT !.b = @ + x.a]
Inv == x.b < 45
====
