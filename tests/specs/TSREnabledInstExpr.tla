---- MODULE TSREnabledInstExpr ----
\* ENABLED within an instance that substitutes expressions for variables:
\* a' and b' are quantified over (a' = 5 is enabled although a is z * 2),
\* in the next-state relation and in an invariant.
EXTENDS Naturals
VARIABLE z, w
I == INSTANCE TSREnabledInstExprA WITH a <- z * 2, b <- <<z, w>>
Init == z = 0 /\ w = 0
Next == \/ I!P /\ z < 3 /\ z' = z + 1 /\ w' = w
        \/ I!R /\ w < 2 /\ w' = w + 1 /\ z' = z
Inv == I!T /\ I!S /\ I!Q
====
