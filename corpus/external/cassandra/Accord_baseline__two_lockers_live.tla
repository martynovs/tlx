---- MODULE Accord_baseline__two_lockers_live ----
\* Generated with matrix.py's gen() of apache/cassandra formalise/accord/execution/tla (Apache-2.0): profile baseline, topology two-lockers, with --liveness.
EXTENDS AccordExec
MCTaskTxns == <<{"c1"}, {"c1"}>>
MCTaskKeys == <<{"k1", "k2"}, {"k1", "k2"}>>
MCTaskParent == <<0, 0>>
====
