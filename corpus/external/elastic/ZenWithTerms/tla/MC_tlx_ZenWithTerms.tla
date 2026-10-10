---- MODULE MC_tlx_ZenWithTerms ----
\* tlx corpus: the TLA+ Toolbox model ZenWithTerms___model.launch of ZenWithTerms.tla as a TLC configuration (ours).
EXTENDS ZenWithTerms, TLC
CONSTANTS n1, n2, n3, v1, v2
tlx_symm_Nodes == Permutations(Nodes)
tlx_symm_Values == Permutations(Values)
tlx_def_Terms ==
    {0,1,2}
tlx_def_Versions ==
    {0,1,2,3}
tlx_def_InitialVersions ==
    {0,1,2}
tlx_constraint ==
    StateConstraint
tlx_symmetry == tlx_symm_Nodes \cup tlx_symm_Values
====
