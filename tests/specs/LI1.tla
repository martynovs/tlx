---- MODULE LI1 ----
\* INSTANCE inside LET: substitutions referring to LET definitions, operator
\* parameters and bound variables; nested parameterized instances;
\* operator-valued instance parameters (LAMBDA).
\* (SANY rejects LET INSTANCE of a nested module, so LI1Counter and LI1Inner
\* are separate files.)
EXTENDS Naturals

---- MODULE LInner2 ----
EXTENDS Naturals
CONSTANT c
B == c + 1
====

---- MODULE LInner1 ----
EXTENDS Naturals
CONSTANT k
J(n) == INSTANCE LInner2 WITH c <- n + k
A(n) == J(n)!B
====

---- MODULE LInner3 ----
CONSTANT F(_)
B == F(1)
====

VARIABLE y

\* Implicit substitution x <- x where x is a LET definition.
Step == LET x == y + 1
            M == INSTANCE LI1Counter
        IN  M!xplus1 - 1

\* Substitution referring to an operator parameter.
Twice(p) == LET M == INSTANCE LI1Inner WITH c <- p IN M!A

\* ... and to a bound variable.
Doubled == {LET M == INSTANCE LI1Inner WITH c <- i IN M!A : i \in 1..3}

\* ... and to an earlier LET definition, through a parameterized instance.
Shifted == LET d == y * 10
               M(e) == INSTANCE LI1Inner WITH c <- d + e
           IN  M(1)!A

I(m) == INSTANCE LInner1 WITH k <- m
Nested == I(10)!J(5)!B
NestedOp == I(10)!A(7)

K(G(_)) == INSTANCE LInner3 WITH F <- G
OpParam == K(LAMBDA z : z + 41)!B

Init == y = 0
Next == y' = (Step + 1) % 4

Inv == /\ Twice(y) = 2 * y
       /\ Doubled = {2, 4, 6}
       /\ Shifted = 20 * y + 2
       /\ Nested = 16
       /\ NestedOp = 18
       /\ OpParam = 42
====
