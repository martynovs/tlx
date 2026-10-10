---- MODULE DyrReduce3 ----
\* DyadicRationals: reducing [num |-> -2^31, den |-> 0] overflows (TLC's
\* Reduce override: commons-math gcd(-2^31, 0) is 2^31, an error).
EXTENDS DyadicRationals, Integers
VARIABLE x
Init == x = 1
Next == x > 0 /\ x' = x - 1
Inv == x > 0 \/ Half([num |-> -2147483647 - x - 1, den |-> x]).den = 0
====
