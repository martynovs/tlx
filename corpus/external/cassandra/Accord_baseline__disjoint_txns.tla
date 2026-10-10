---- MODULE Accord_baseline__disjoint_txns ----
\* Generated with matrix.py's gen() of apache/cassandra formalise/accord/execution/tla (Apache-2.0): profile baseline, topology disjoint-txns.
EXTENDS AccordExec
MCTaskTxns == <<{"c1"}, {"c2"}>>
MCTaskKeys == <<{"k1", "k2"}, {"k1", "k2"}>>
MCTaskParent == <<0, 0>>
====
