---- MODULE LF11 ----
\* Fairness of an action through an INSTANCE that substitutes an expression
\* for a variable the action primes (compat.md D-8). Act is
\* d' = FALSE /\ c' = 1: within the instance its ENABLED quantifies over d'
\* and c', so it holds everywhere although the spec never takes such a
\* step: Prop is violated (TLC and tlx).
EXTENDS Naturals
VARIABLES a
Init == a = 0
Next == a < 2 /\ a' = a + 1
I == INSTANCE LF11Inner WITH d <- a # 0, c <- a
Spec == Init /\ [][Next]_a
Prop == I!ISpec
====
