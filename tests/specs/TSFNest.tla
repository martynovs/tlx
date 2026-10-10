---- MODULE TSFNest ----
\* Contract: a nested module sees the declarations and definitions of its
\* enclosing module that precede it (SANY), also when the enclosing module
\* instantiates it itself (anonymously or by name, with substitutions for
\* the nested module's own parameters). TLC's SwarmKit benchmark needs it.
EXTENDS Naturals
CONSTANT N
VARIABLE x
K == N + 1
---- MODULE Sub ----
VARIABLE y
Step == x < K /\ x' = x + 1 /\ y' = y
Get == y + K
====
VARIABLE z
S == INSTANCE Sub WITH y <- z
INSTANCE Sub WITH y <- z
Init == x = 0 /\ z = 7
Next == S!Step /\ Step
Inv == S!Get = 7 + K /\ Get = S!Get
====
