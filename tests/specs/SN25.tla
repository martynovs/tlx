---- MODULE SN25 ----
\* x \in {f : y \in S}. TLC evaluates x before it builds the set, so
\* an erroring x is reported even when S is empty: the compiled filter over
\* an empty S never reaches the equality, and reported a false invariant
\* instead. s holds a set of integers that is empty in the checked state.
EXTENDS Integers, Sequences
VARIABLES n, s
Init == n = 0 /\ s = {1, 2}
Next == n < 2 /\ n' = n + 1 /\ s' = s \ s
Inv == IF n = 0 THEN TRUE ELSE <<1, 2>>[5] \in {y : y \in s}
====
