---- MODULE OvrEntryMod ----
\* Named instances, with and without substitutions, of an extended module
\* and of one without declarations: a direct reference I!D to an overridden
\* definition evaluates the original body, and the bodies of the instance
\* (I!Twice, B!Next) see the override (docs/compat.md D-23).
EXTENDS OvrEBase, OvrEConst
B == INSTANCE OvrEBase
BW == INSTANCE OvrEBase WITH N <- 2
C == INSTANCE OvrEConst
MyOk(v) == B!Ok(v) /\ v # 2
MyLim == BW!Lim + 100
MyK == C!K + 10
ASSUME <<Lim, Twice, B!Lim, B!Twice, BW!Lim, BW!Twice>> = <<103, 206, 3, 206, 3, 206>>
ASSUME <<K, TwiceK, C!K, C!TwiceK>> = <<13, 26, 3, 26>>
ASSUME <<Ok(3), B!Ok(3), BW!Ok(3), BW!Ok(1)>> = <<TRUE, TRUE, FALSE, TRUE>>
Spec == Init /\ [][B!Next]_x
====
