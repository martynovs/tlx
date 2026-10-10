---- MODULE MC_tlx_replication ----
\* tlx corpus: the TLA+ Toolbox model replication___model.launch of replication.tla as a TLC configuration (ours).
EXTENDS replication, TLC
CONSTANTS n1, n2, n3, id1, id2
tlx_symm_Nodes == Permutations(Nodes)
tlx_symm_DocumentIds == Permutations(DocumentIds)
tlx_constraint ==
    StateConstraint
tlx_inv_1 ==
    WellFormedRoutingTable(clusterStateOnMaster.routingTable)
tlx_prop_0 ==
    <>AllCopiesSameContents
tlx_prop_1 ==
    <>NoActiveMessages
tlx_prop_2 ==
    <>AllAckedResponsesStored
tlx_symmetry == tlx_symm_Nodes \cup tlx_symm_DocumentIds
====
