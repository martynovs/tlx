---- MODULE FlexibleRaftReduced5 ----
EXTENDS FlexibleRaft

\* Bound the election and replication message combinations without changing
\* the five-server symmetry group or the VIEW representative selection.
MessageBound == Cardinality(DOMAIN messages) <= 16
====
