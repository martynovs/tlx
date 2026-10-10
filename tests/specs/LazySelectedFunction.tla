---- MODULE LazySelectedFunction ----
EXTENDS Integers, TLC, Functions
VARIABLE x
Init == x = 0
Next == /\ x < 2
        /\ x' = x + 1
Inv == DOMAIN (<<Restrict((1 :> x), {3})>>)[1] = {3}
====
