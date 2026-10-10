---- MODULE LzRecCycleInit ----
\* The cfg redirects Init to ReDefInit, which refers to Init: the definitions
\* refer to each other forever. TLC reports a StackOverflowError; tlx must
\* report an evaluation error rather than crash (also while classifying the
\* specification, which looks through 0-ary definitions).
VARIABLE x, z
vars == <<x>>
Init == x = FALSE
Next == x' = ~x /\ z' = z
Spec == Init /\ [][Next]_<<x, z>>
ReDefInit == x \in BOOLEAN /\ z = TRUE /\ Init
====
