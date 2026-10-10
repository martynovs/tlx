---- MODULE TSREnabledInst2 ----
\* TLC's test57: I!D (ENABLED A within the instance) holds, I!B(I!A)
\* (ENABLED applied to the substituted action) does not; I!C = I!B(I!A)
\* passes A as an operator argument, which TLC evaluates with the
\* substitutions applied, so the first invariant violated is Invariant1.
EXTENDS Naturals
VARIABLE x
I == INSTANCE TSREnabledInstA WITH x <- x, y <- x
Init == \E i \in 1..3 : x = i
Next == x' = 9 - x
Spec == Init /\ [][Next]_x
Invariant0 == I!B(Next)
Invariant1 == I!C
Invariant2 == ~ I!B(I!A)
Invariant3 == I!D
====
