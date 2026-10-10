---- MODULE TSREarlySafety ----
\* A temporal property violated by a finite behavior (its tableau reaches an
\* accepting sink): TLC reports it during the search, as a temporal
\* violation with the exit status of a safety violation (12).
VARIABLE x
Init == x = FALSE
Next == x' \in BOOLEAN
Spec == Init /\ [][Next]_x
AtMostOnce == [](x => [](~x => []~x))
====
