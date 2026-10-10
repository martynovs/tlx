---- MODULE MC_tlx_consensus ----
\* tlx corpus: the TLA+ Toolbox model consensus___model.launch of consensus.tla as a TLC configuration (ours).
EXTENDS consensus, TLC
CONSTANTS n1, n2, n3, v1, v2
tlx_symm_Nodes == Permutations(Nodes)
tlx_symm_Values == Permutations(Values)
tlx_constraint ==
    StateConstraint
tlx_symmetry == tlx_symm_Nodes \cup tlx_symm_Values
====
