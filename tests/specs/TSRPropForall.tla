---- MODULE TSRPropForall ----
\* A PROPERTY universally quantified over a constant set: TLC unrolls it
\* (SpecProcessor.processConfigProps) and checks [][A]_v as an action
\* property named by the location of the quantifier's body.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 4
Spec == Init /\ [][Next]_x
ActP == \A k \in {1, 2} : [][x' > x \/ k = 1]_x
====
