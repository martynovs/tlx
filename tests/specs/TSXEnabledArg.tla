---- MODULE TSXEnabledArg ----
\* ENABLED of an operator whose argument is primed inside it, evaluated in
\* a state context (an invariant): the argument is the expression `y`, so
\* `A'` is `y'` (TLC's test20). And ENABLED in Init, over the initial state
\* being built (test216).
EXTENDS Naturals, TLC
VARIABLE x, y
Foo(A) == ENABLED ((x' = x) /\ (A' = x + 1))
Baz(A) == ENABLED (A' = x + 1)
Init == /\ x \in 0..2
        /\ y = 1
        /\ ENABLED (x' = x + 1)
        /\ (x = 0 \/ ENABLED (y' = x))
Next == /\ x < 3
        /\ x' = x + 1
        /\ y' = y
Inv == /\ Foo(y)
       /\ ~Foo(x)
       /\ Baz(y)
       /\ LET A == x IN ~Foo(A)
====
