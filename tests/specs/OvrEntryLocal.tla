---- MODULE OvrEntryLocal ----
\* A LOCAL INSTANCE gives the root module nodes of its own even for a
\* module without declarations: an override of K does not reach TwiceK
\* (docs/compat.md D-23).
EXTENDS Naturals
VARIABLE x
LOCAL INSTANCE OvrEConst
ASSUME <<K, TwiceK>> = <<8, 6>>
Spec == x = 0 /\ [][x' = (x + 1) % TwiceK]_x
====
