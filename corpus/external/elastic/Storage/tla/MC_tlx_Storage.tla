---- MODULE MC_tlx_Storage ----
\* tlx corpus: the TLA+ Toolbox model Storage___model.launch of Storage.tla as a TLC configuration (ours).
EXTENDS Storage, TLC
tlx_const_MaxNewMeta ==
    5
tlx_constraint ==
    newMeta < MaxNewMeta
====
