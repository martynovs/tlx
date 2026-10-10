---- MODULE Accord_ctl_no_upgrade__two_lockers ----
\* Generated with matrix.py's gen() of apache/cassandra formalise/accord/execution/tla (Apache-2.0): profile ctl-no-upgrade, topology two-lockers.
EXTENDS AccordExec
MCTaskTxns == <<{"c1"}, {"c1"}>>
MCTaskKeys == <<{"k1", "k2"}, {"k1", "k2"}>>
MCTaskParent == <<0, 0>>
====
