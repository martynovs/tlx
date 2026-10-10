---- MODULE RN1 ----
\* Random choices in constant definitions are made once, at startup, from
\* java.util.Random seeded with -seed: with the same seed tlx makes TLC's
\* choices (RandomElement, RandomSubset, RandomSetOfSubsets).
EXTENDS Integers, FiniteSets, TLC, Randomization
VARIABLES reach, tag
Nodes == 1..6
Edges == UNION {{{n, m} : m \in RandomElement(SUBSET (Nodes \ {n}))} : n \in Nodes}
Sub == RandomSubset(3, 1..20)
Sets == RandomSetOfSubsets(3, 2, Nodes)
Rec == RandomElement([a : 1..5, b : {"x", "y"}])
Starts == ((Sub \cup UNION Sets) \cap Nodes) \cup {Rec.a}
Init == reach \in {{n} : n \in Starts} /\ tag = Rec.b
Next == \E e \in Edges :
          /\ e \cap reach /= {}
          /\ ~(e \subseteq reach)
          /\ reach' = reach \cup e
          /\ tag' = tag
====
