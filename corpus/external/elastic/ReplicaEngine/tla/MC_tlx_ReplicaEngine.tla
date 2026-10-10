---- MODULE MC_tlx_ReplicaEngine ----
\* tlx corpus: the TLA+ Toolbox model ReplicaEngine___model.launch of ReplicaEngine.tla as a TLC configuration (ours).
EXTENDS ReplicaEngine, TLC
CONSTANTS DocA, DocB
tlx_symm_DocContent == Permutations(DocContent)
tlx_const_DocAutoIdTimestamp ==
    1000
tlx_const_DuplicationLimit ==
    1
tlx_symmetry == tlx_symm_DocContent
====
