---- MODULE OvrInstDirect ----
\* A direct reference I!Start to an overridden definition of an instance
\* evaluates the original body (docs/compat.md D-23).
EXTENDS OvrInst
Direct == I!Start \/ x > 1
====
