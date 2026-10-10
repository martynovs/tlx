---- MODULE TSRNoInitTrue ----
\* An initial predicate that is a constant definition valued TRUE is
\* dropped (TLC looks the name up as its value); with an invariant left to
\* check, the configuration lacks an initial predicate.
EXTENDS Naturals
Init == TRUE
Next == TRUE
Spec == Init /\ [][Next]_<<>>
Inv == 1 + 1 = 2 /\ ENABLED TRUE
====
