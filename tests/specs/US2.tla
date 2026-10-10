---- MODULE US2 ----
\* The order in which TLC evaluates constant-level definitions at startup
\* (SpecProcessor.processConstantDefns), seen through the order of the
\* strings they create: the modules in semantic order (US2A, which
\* is extended, and US2C, instantiated without parameters, before US2); in
\* each, first the definitions substituted for constants (K2 <- DefK), then
\* the 0-ary definitions in declaration order, where a named INSTANCE
\* contributes its definitions (I!B1, of a module with parameters that is
\* not processed on its own) at its position. LOCAL definitions count too.
EXTENDS Naturals, TLC, US2A
CONSTANT K2
VARIABLE v
S(n) == ToString(n + 1000)
R2 == S(2)
I == INSTANCE US2B WITH K <- 5
R1 == S(1)
J == INSTANCE US2C
LOCAL R0 == S(3)
DefK == S(50)
Init == v = {S(9), S(8)} \cup {R1, R2, R0, A1, A2, S(7), I!B1, J!C1, K2}
Next == UNCHANGED v
Inv == v = {}
====
