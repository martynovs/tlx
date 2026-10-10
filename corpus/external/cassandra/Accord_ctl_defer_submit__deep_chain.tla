---- MODULE Accord_ctl_defer_submit__deep_chain ----
\* Generated with matrix.py's gen() of apache/cassandra formalise/accord/execution/tla (Apache-2.0): profile ctl-defer-submit, topology deep-chain.
EXTENDS AccordExec
MCTaskTxns == <<{}, {}, {}, {}>>
MCTaskKeys == <<{"k1"}, {"k1"}, {"k1"}, {"k1"}>>
MCTaskParent == <<0, 0, 2, 3>>
====
