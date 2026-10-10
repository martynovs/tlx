---- MODULE TSFCfgOps ----
\* Contract: the names of a cfg substitution may be operator symbols
\* (TLC's suite test30): `++ <- PlusPlus` replaces a declared infix
\* constant, `Plus <- +` binds an operator constant to a standard operator,
\* `\o <- +` overrides a native standard-module operator by another one.
EXTENDS Naturals, Sequences
CONSTANT _++_, Plus(_, _)
VARIABLE x
PlusPlus(a, b) == <<a, b>>
Init == x = 1
Next == x' = x
Inv == /\ (2 ++ (x + 3)) = <<2, 4>>
       /\ Plus(2, x + 3) = 6
       /\ 1 \o 2 = 3
       /\ 1 \circ 2 = 3
====
