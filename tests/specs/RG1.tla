---- MODULE RG1 ----
\* Runaway recursion: F has no base case, so evaluating it never returns.
\* TLC runs out of JVM stack and reports a StackOverflowError
\* (an error, exit 75); tlx must report an evaluation error too, rather
\* than crash with a native stack overflow.
EXTENDS Naturals
RECURSIVE F(_)
F(n) == F(n + 1) + 1
VARIABLE x
Init == x = F(0)
Next == UNCHANGED x
====
