---- MODULE EK6 ----
\* An action property that cannot be evaluated on a step: "Evaluating
\* action property AP failed." (2113, exit 75) in every tier, with the
\* step's successor ending the trace.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 5
AP == [][<<1, 2>>[x' + 1] > 0]_x
====
