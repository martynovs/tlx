---- MODULE OvrEntryVal ----
\* `D = value` for a definition: like `D <- E`, the bodies of every
\* instance see the value, a direct reference I!D (also through a LET
\* INSTANCE) the original body (docs/compat.md D-23).
EXTENDS OvrEBase, OvrEConst
B == INSTANCE OvrEBase
C == INSTANCE OvrEConst
ASSUME <<Lim, Twice, B!Lim, B!Twice>> = <<8, 16, 3, 16>>
ASSUME <<K, TwiceK, C!K, C!TwiceK, LET L == INSTANCE OvrEConst IN L!K>> = <<7, 14, 3, 14, 3>>
Spec == Init /\ [][Next]_x
====
