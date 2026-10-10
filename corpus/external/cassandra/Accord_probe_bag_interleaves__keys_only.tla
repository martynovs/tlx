---- MODULE Accord_probe_bag_interleaves__keys_only ----
\* Generated with matrix.py's gen() of apache/cassandra formalise/accord/execution/tla (Apache-2.0): profile probe-bag-interleaves, topology keys-only.
EXTENDS AccordExec
MCTaskTxns == <<{"c1"}, {}, {}>>
MCTaskKeys == <<{"k1", "k2"}, {"k1", "k2"}, {"k1"}>>
MCTaskParent == <<0, 0, 0>>
====
