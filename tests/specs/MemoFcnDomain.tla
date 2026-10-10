---- MODULE MemoFcnDomain ----
\* The source region reads n and produces equal function pairs with
\* different domain witnesses. The body region only reads its boxed f
\* input: a cached successful EXCEPT on the enumerated domain must not
\* hide the interval-domain type error in the second state.
\* Keep the range state-dependent to avoid constant-region equivalence.
EXTENDS Integers, TLC
CONSTANT absent
VARIABLE n
Init == n = 0
Next == \E f \in (IF n = 0 THEN [{0} -> {7+n-n}] ELSE [0..0 -> {7+n-n}]):
          /\ [f EXCEPT ![absent] = 8] = f
          /\ n' = 1
====
