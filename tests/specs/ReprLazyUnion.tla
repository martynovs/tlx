---- MODULE ReprLazyUnion ----
\* A lazy union enumerates left then right with duplicates: the successor
\* {} is generated twice (13 generated / 3 distinct, where the equal
\* explicit set {{}, {1}, {2}} generates 10). Normalizing the source would
\* change the generated count (docs/design.md §5.1).
EXTENDS Naturals
VARIABLE x
Init == x = {}
Next == x' \in ((SUBSET {1}) \cup (SUBSET {2}))
====
