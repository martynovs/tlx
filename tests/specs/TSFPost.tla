---- MODULE TSFPost ----
\* Contract: POSTCONDITION takes several names (also over several lines);
\* TLC checks them in order and reports the first that is false
\* (Tool.checkPostConditions; TLC tests PostConditionsTest/FailTest).
EXTENDS Naturals, TLC
VARIABLE x
Init == x = 0
Next == x < 3 /\ x' = x + 1
Post1 == TLCGet("distinct") = 4
Post2 == TLCGet("distinct") = 42
Post3 == TLCGet("distinct") = 0
====
