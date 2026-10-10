---- MODULE SN2 ----
\* IsFiniteSet of an encoded set must still evaluate its operand
\* (TLC reports the out-of-domain application v[3]); the compiled tier
\* folded it to TRUE. The small pre-run (SN2.flags) keeps the c = 3 branch
\* out of the shape pre-run, so the action is lowered with typed layouts.
EXTENDS Naturals, FiniteSets
VARIABLE v, r, c
Init == v = <<1, 2>> /\ r = FALSE /\ c \in 0..3
Next == /\ r' = IF c = 3 THEN IsFiniteSet({v[c]}) ELSE FALSE
        /\ UNCHANGED <<v, c>>
====
