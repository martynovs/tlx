---- MODULE TSFOpTable ----
\* Contract: the cfg may give an operator constant by its values at some
\* arguments, `F(0) = 1` (TLC's OpRcdValue; TLC test
\* ConstantOperatorConfiguration): here Inv is violated after two steps.
EXTENDS Naturals
CONSTANT F(_), G(_, _)
VARIABLE x
Init == x = 0
Next == x' = F(x)
Inv == x /= 2 /\ G(x, "a") = {x}
====
