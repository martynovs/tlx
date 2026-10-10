---- MODULE DyrReduce2 ----
\* DyadicRationals: reducing [num |-> 0, den |-> 0] divides by zero (TLC's
\* Reduce override: commons-math gcd(0, 0) = 0).
EXTENDS DyadicRationals, Integers
VARIABLE x
Init == x = 2
Next == x > 0 /\ x' = x - 1
Inv == x > 0 \/ Half([num |-> 0, den |-> x]) = Zero
====
