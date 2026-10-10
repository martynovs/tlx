---- MODULE LzRecTemporalLoop ----
\* A recursive operator with a temporal body whose constant IF guard never
\* ends the recursion, and one whose guard depends on the state (so both
\* branches are translated, forever). TLC reports a StackOverflowError;
\* tlx must report an error rather than run forever or crash.
EXTENDS Naturals
VARIABLE x
RECURSIVE bad(_)
bad(u) == IF u THEN [](x \in 0..3) ELSE bad(u)
RECURSIVE op(_)
op(n) == IF n = 0 THEN <>(x = 0) ELSE op(n - 1)
Init == x = 0
Next == x' \in 0..3
Spec == Init /\ [][Next]_x
PropLoop == bad(FALSE)
PropState == op(x)
====
