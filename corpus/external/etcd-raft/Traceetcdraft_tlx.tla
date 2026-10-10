------------------------- MODULE Traceetcdraft_tlx -------------------------
\* (tlx corpus wrapper.) Traceetcdraft without environment variables: the
\* repository's example log, whole or its first 400 lines (the cfgs override
\* JsonFile and TraceLog).
EXTENDS Traceetcdraft

TlxJsonFile == "./example.ndjson"

TlxTraceLog400 == TLCEval(SubSeq(OriginTraceLog, 1, 400))
=============================================================================
