---- MODULE TSFPossibleOk ----
\* Contract: `_POSSIBLE` predicates that all hold somewhere (a state, a
\* step) do not fail the run (TLC test PossibleTest).
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
AllDone == x = 2
WrapAround == x = 2 /\ x' = 0
====
