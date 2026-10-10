---- MODULE Accord_baseline__keys_only ----
\* Generated with matrix.py's gen() of apache/cassandra formalise/accord/execution/tla (Apache-2.0): profile baseline, topology keys-only.
EXTENDS AccordExec
MCTaskTxns == <<{"c1"}, {}, {}>>
MCTaskKeys == <<{"k1", "k2"}, {"k1", "k2"}, {"k1"}>>
MCTaskParent == <<0, 0, 0>>
====
