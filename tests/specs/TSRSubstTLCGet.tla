---- MODULE TSRSubstTLCGet ----
\* The same with TLCGet: its statistics are undefined at startup.
EXTENDS TLC
diameter == TLCGet("diameter")
CONSTANT A
ASSUME TRUE
====
