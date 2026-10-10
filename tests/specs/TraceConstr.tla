---- MODULE TraceConstr ----
\* TLCExt!Trace in a CONSTRAINT is the path to the successor checked: its
\* predecessor's path and the successor (docs/compat.md D-24: TLC drops the
\* predecessor's path there).
EXTENDS TraceOp

Constr == Len(Trace) <= 5
====
