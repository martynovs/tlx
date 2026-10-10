---- MODULE SN1 ----
\* Sets of the empty set held in state variables (shape
\* Set { elem: Never, max_card: 0 }, an encoding with zero words). Ranking
\* such an element indexed out of bounds in the tree-walking IR executor and
\* read a neighbouring word in the closure executor / C / JIT.
EXTENDS Naturals, Sequences, FiniteSets
ToSet(s) == {s[i] : i \in DOMAIN s}
VARIABLE w, v, u, x
Init == w = 0 /\ v = <<{}>> /\ u = {{}} /\ x = 0
Next == /\ w < 3
        /\ w' = w + 1
        /\ v' = v
        /\ u' = u \cap {{}}
        /\ x' = Cardinality(UNION ToSet(v)) + Cardinality(LET S2 == {{}} \cap u IN CHOOSE e \in S2 : TRUE)
Inv == /\ UNION ToSet(v) = {}
       /\ ToSet(v) = {{}}
       /\ {} \in ToSet(v)
       /\ v[1] \in u
       /\ Cardinality(ToSet(v)) = 1
       /\ (CHOOSE e \in u : TRUE) = {}
       /\ (LET S2 == {{}} \cap u IN CHOOSE e \in S2 : TRUE) = {}
       /\ x = 0
====
