---- MODULE SN11 ----
\* TLCExt!TLCGetOrDefault is native in TLC (its definition is an
\* unbounded CHOOSE), and TLCGetAndSet is defined with it. Integer and
\* named ("s:...") registers, set in ASSUMEs and in the next-state
\* relation; the number of distinct states depends on the values read.
\* A 0-ary constant definition using it is evaluated once at startup.
EXTENDS Naturals, TLC, TLCExt
D == TLCGetOrDefault(3, 7)
ASSUME TLCGetOrDefault(1, "none") = "none"
ASSUME TLCGetOrDefault("s:a", 5) = 5
ASSUME TLCGetOrDefault("level", 5) = 5
ASSUME TLCSet(1, 10)
ASSUME TLCGetOrDefault(1, "none") = 10
ASSUME TLCGetAndSet(2, +, 1, 100) = 100
ASSUME TLCGetOrDefault(2, 0) = 101
ASSUME TLCGetAndSet("s:b", +, 3, 0) = 0
ASSUME TLCGet("s:b") = 3
ASSUME TLCSet(3, 1)
VARIABLE x, seen
Init == x = 0 /\ seen = 0
Next == /\ x < 4
        /\ x' = x + 1
        /\ \E i \in 1..2 : seen' = (TLCGetAndSet("s:c", +, i, 0) + TLCGetAndSet(4, +, 2, 0) + D) % 5
Inv == seen < 5 /\ D = 7 /\ TLCGetOrDefault(1, 0) \in {0, 10}
====
