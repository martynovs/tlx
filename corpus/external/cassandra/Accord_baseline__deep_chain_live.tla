---- MODULE Accord_baseline__deep_chain_live ----
\* Generated with matrix.py's gen() of apache/cassandra formalise/accord/execution/tla (Apache-2.0): profile baseline, topology deep-chain, with --liveness.
EXTENDS AccordExec
MCTaskTxns == <<{}, {}, {}, {}>>
MCTaskKeys == <<{"k1"}, {"k1"}, {"k1"}, {"k1"}>>
MCTaskParent == <<0, 0, 2, 3>>
====
