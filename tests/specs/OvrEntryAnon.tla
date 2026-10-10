---- MODULE OvrEntryAnon ----
\* An anonymous INSTANCE of a module with parameters gives the root module
\* nodes of its own: an override of one (Ok, Lim) binds only the root's
\* name, not the module's bodies (Next and Twice keep the original). The
\* definitions of a module without declarations stay shared (TwiceK sees
\* K's value). docs/compat.md D-23.
EXTENDS Naturals
VARIABLE x
INSTANCE OvrEBase WITH N <- 5
INSTANCE OvrEConst
B == INSTANCE OvrEBase WITH N <- 5
MyOk(v) == v < 2
ASSUME <<Lim, Twice, K, TwiceK, B!Lim, B!Twice>> = <<7, 6, 8, 16, 3, 6>>
ASSUME <<Ok(3), B!Ok(3)>> = <<FALSE, TRUE>>
Spec == Init /\ [][Next]_x
====
