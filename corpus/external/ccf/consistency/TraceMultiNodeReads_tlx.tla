---------------------- MODULE TraceMultiNodeReads_tlx ----------------------
\* (tlx corpus wrapper.) TraceMultiNodeReads on the traces of the
\* repository, without environment variables (the cfgs override JsonFile).
EXTENDS TraceMultiNodeReads

TlxElectionNoRollback == "traces/election_no_rollback.ndjson"
TlxElectionNonContiguousView == "traces/election_non_contiguous_view.ndjson"
TlxRollbackSameSeqno == "traces/rollback_same_seqno.ndjson"
TlxRollbackTwoTransactions == "traces/rollback_two_transactions.ndjson"
=============================================================================
