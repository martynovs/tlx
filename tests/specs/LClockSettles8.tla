---- MODULE LClockSettles8 ----
EXTENDS Naturals
VARIABLE hr
Init == hr = 1
Next == hr' = IF hr = 8 THEN 1 ELSE hr + 1
Spec == Init /\ [][Next]_hr /\ WF_hr(Next)
ClockSettles == \E k \in 1..8: <>[](hr = k)
====
