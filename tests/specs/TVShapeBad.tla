---- MODULE TVShapeBad ----
\* TVShape with a log that has a wrong counter value: the trace is matched
\* only up to that line (POSTCONDITION false). The log is replaced by a cfg
\* override.
EXTENDS TVShape
BadLog == CausalOrder(ndJsonDeserialize("TVShapeBad.ndjson"),
                      LAMBDA l : l.vc, LAMBDA l : ToString(l.node), LAMBDA vc : DOMAIN vc)
====
