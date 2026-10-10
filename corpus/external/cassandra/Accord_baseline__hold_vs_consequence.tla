---- MODULE Accord_baseline__hold_vs_consequence ----
\* Generated with matrix.py's gen() of apache/cassandra formalise/accord/execution/tla (Apache-2.0): profile baseline, topology hold-vs-consequence.
EXTENDS AccordExec
MCTaskTxns == <<{"c1"}, {"c1"}>>
MCTaskKeys == <<{"k1", "k2"}, {"k1"}>>
MCTaskParent == <<0, 1>>
====
