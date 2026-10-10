---- MODULE NeonMaxEmpty ----
\* Max of an encoded set that becomes empty: an evaluation error (the
\* compiled tier leaves it to the interpreter).
EXTENDS Naturals, FiniteSetsExt
VARIABLES s
Init == s = {1, 2}
Next == \E x \in s : s' = s \ {x}
Inv == Max(s) > 0
====
