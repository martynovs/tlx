---- MODULE RecordSetOrder ----
EXTENDS Naturals, FiniteSets
VARIABLE r
Init == r = [generation |-> 1, members |-> {1, 4}]
Candidates == {r} \cup {[generation |-> 1, members |-> {2, 3}]}
Next == r' = CHOOSE c \in Candidates: TRUE
TypeOk == r \in [generation: 0..10000, members: SUBSET (1..4)]
Correct == r.members = {1, 4}
====
