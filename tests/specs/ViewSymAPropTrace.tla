---- MODULE ViewSymAPropTrace ----
EXTENDS Naturals, TLC
CONSTANT Proc
VARIABLES owner, hidden
vars == <<owner, hidden>>
Init == owner \in Proc /\ hidden = 0
Next == \/ UNCHANGED vars
        \/ /\ hidden' = 1
           /\ UNCHANGED owner
Spec == Init /\ [][Next]_vars
AProp == [][FALSE]_vars
View == owner
Sym == Permutations(Proc)
====
