---- MODULE SymCheckChoose ----
EXTENDS TLC
CONSTANT Nodes
VARIABLE x
Init == x = {}
Pick == x' = {CHOOSE n \in Nodes: TRUE}
Next == Pick
Sym == Permutations(Nodes)
Inv == x \subseteq Nodes
====
