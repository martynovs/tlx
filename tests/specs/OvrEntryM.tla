---- MODULE OvrEntryM ----
\* The module-scoped forms `D = [M] v` and `D <- [M] E` override the body,
\* which every instance shares: direct references I!D see them too
\* (docs/compat.md D-23).
EXTENDS OvrEBase
B == INSTANCE OvrEBase
MyOk(v) == v < 2
ASSUME <<Lim, Twice, B!Lim, B!Twice, LET L == INSTANCE OvrEBase IN L!Lim>> = <<40, 80, 40, 80, 40>>
ASSUME <<Ok(1), Ok(3), B!Ok(3), LET L == INSTANCE OvrEBase IN L!Ok(3)>> = <<TRUE, FALSE, FALSE, FALSE>>
Spec == Init /\ [][B!Next]_x
====
