---- MODULE OvrEntryLet ----
\* The replacement of an overridden definition calls the original through a
\* LET INSTANCE of its module (tlaplus/Examples' MCCheckpointCoordination):
\* CC!Ok is a node of its own, which the override does not reach, while the
\* bodies of the module (Next) call the replacement (docs/compat.md D-23).
\* x reaches N + 1, violating Bounded.
EXTENDS OvrEBase
WeakOk(v) == LET CC == INSTANCE OvrEBase IN CC!Ok(v) \/ v = N
Spec == Init /\ [][Next]_x
Bounded == x <= N
====
