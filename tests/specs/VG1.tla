---- MODULE VG1 ----
\* A set variable that is always {}: its bitset has an empty universe and
\* the state has 0 words. Membership of a constant outside that universe
\* must not read the bitset (the JIT's branchless bit test read word 0 of
\* the 0-word state, a dangling pointer: SIGSEGV).
EXTENDS Naturals
VARIABLE E
Init == E = {}
Next == E' = E \cap {1}
Inv == 2 \notin E /\ (1 \in E => 3 \in E)
====
