---- MODULE SN26 ----
\* x \in {f : y \in S} maps only the elements of S, so f is never
\* evaluated when S is empty (TLC reports no error; the erroring f here
\* would be one). Companion of SN25 under the same empty, integer-typed s.
EXTENDS Integers, Sequences
VARIABLES n, s
Init == n = 0 /\ s = {1, 2}
Next == IF n < 2 THEN n' = n + 1 /\ s' = s \ s ELSE UNCHANGED <<n, s>>
Inv == IF n = 0 THEN TRUE ELSE ~(1 \in {<<1, 2>>[5] : y \in s})
====
