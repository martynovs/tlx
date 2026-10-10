---- MODULE SN14 ----
\* A function TLC builds over an integer interval lo..hi with
\* lo # 1 ([k \in 0..2 |-> e], the elements of [0..2 -> S]) is an
\* FcnRcdValue with an interval domain: a key that is not an integer is an
\* error, also for EXCEPT, where the same function over the enumerated set
\* {0, 1, 2} returns itself unchanged (SN3, SN5). Here f (built over 0..2,
\* a state variable updated with EXCEPT) gets a model-value key in the
\* third state; the ASSUMEs check the other constructions.
EXTENDS Naturals, TLC
CONSTANTS M
ASSUME [[k \in {0, 1} |-> 0] EXCEPT ![M] = 1] = [k \in {0, 1} |-> 0]
ASSUME [(CHOOSE h \in [{0, 1} -> {0}] : TRUE) EXCEPT ![M] = 1] = [k \in 0..1 |-> 0]
ASSUME [(0 :> 0) @@ (1 :> 0) EXCEPT ![M] = 1] = [k \in 0..1 |-> 0]
VARIABLES n, f, g
Init == /\ n = 0
        /\ f = [k \in 0..2 |-> 0]
        /\ g \in [0..1 -> {0, 1}]
Next == /\ n < 3
        /\ n' = n + 1
        /\ f' = IF n < 2 THEN [f EXCEPT ![n] = 1] ELSE [f EXCEPT ![M] = 2]
        /\ g' = [g EXCEPT ![0] = 1 - @]
====
