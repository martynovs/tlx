---- MODULE Accord_ctl_fifo_adopt__consequence_non_subset ----
\* Generated with matrix.py's gen() of apache/cassandra formalise/accord/execution/tla (Apache-2.0): profile ctl-fifo-adopt, topology consequence-non-subset.
EXTENDS AccordExec
MCTaskTxns == <<{"c1"}, {"c1"}, {}>>
MCTaskKeys == <<{"k1", "k2"}, {"k1", "k2", "k3"}, {"k2", "k3"}>>
MCTaskParent == <<0, 1, 0>>
====
