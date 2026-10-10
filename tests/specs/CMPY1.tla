---- MODULE CMPY1 ----
\* A non-recursive operator applied to an application of itself, whose
\* argument the compiled tier lowers lazily (a constructor): no recursion,
\* so it is inlined, not handed to the interpreter.
EXTENDS Naturals
VARIABLES x, y
Wrap(p) == <<p, 1>>
Init == x = 0 /\ y = 0
Next == /\ x < 5
        /\ x' = Wrap(Wrap(x))[1][1] + 1
        /\ y' = Wrap(Wrap(Wrap(y)))[1][2] + x
Inv == x <= 5 /\ Wrap(Wrap(<<x, y>>))[1][1][2] = y
====
