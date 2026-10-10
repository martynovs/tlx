---- MODULE Accord_baseline__two_txns ----
\* Generated with matrix.py's gen() of apache/cassandra formalise/accord/execution/tla (Apache-2.0): profile baseline, topology two-txns.
EXTENDS AccordExec
MCTaskTxns == <<{"c1", "c2"}, {"c1"}, {"c2"}>>
MCTaskKeys == <<{"k1", "k2"}, {"k1", "k2"}, {"k1"}>>
MCTaskParent == <<0, 1, 0>>
====
