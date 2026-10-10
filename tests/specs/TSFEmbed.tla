---- MODULE TSFEmbed ----
\* Contract: `-config TSFEmbed.tla` (TSFEmbed.flags) reads the configuration
\* from the `---- CONFIG TSFEmbed ----` section after the module (TLC's
\* MonolithSpecExtractor), not TSFEmbed.cfg, which names an invariant that
\* is violated.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Inv == x < 3
Wrong == x < 2
====
---- CONFIG Other ----
INIT Init
NEXT Next
INVARIANT Wrong
====
---- CONFIG TSFEmbed ----
INIT Init
NEXT Next
INVARIANT Inv
\* (a comment)
====
