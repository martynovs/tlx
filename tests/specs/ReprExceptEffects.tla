---- MODULE ReprExceptEffects ----
\* EXCEPT on a bag keyed by records with a sequence field (a value store slot
\* under TLX_VALUE_STORE, like Raft's messages): a key outside the domain
\* leaves the function unchanged without evaluating the replacement (no
\* error, no output), also beside a present key of the same EXCEPT; a
\* present key evaluates its replacement once, with its output and `@`.
EXTENDS Naturals, Sequences, TLC
VARIABLES bag, n
K(i) == [id |-> i, log |-> <<i>>]
Init == bag = (K(1) :> 0) @@ (K(2) :> 5) /\ n = 0
Next == \/ n = 0 /\ bag' = [bag EXCEPT ![K(7)] = 1 \div 0] /\ n' = 1
        \/ n = 0 /\ bag' = [bag EXCEPT ![K(8)] = PrintT("absent")] /\ n' = 2
        \/ n = 0 /\ bag' = [bag EXCEPT ![K(1)] = IF PrintT(<<"present", @>>) THEN @ + 1 ELSE @] /\ n' = 3
        \/ n = 0 /\ bag' = [bag EXCEPT ![K(2)] = @ - 1, ![K(9)] = 1 \div 0] /\ n' = 4
        \/ n \in {1, 2} /\ bag' = bag @@ (K(n + 2) :> n) /\ n' = n + 10
Inv == /\ n \in {1, 2} => bag = (K(1) :> 0) @@ (K(2) :> 5)
       /\ n = 3 => bag[K(1)] = 1
       /\ n = 4 => bag[K(2)] = 4 /\ DOMAIN bag = {K(1), K(2)}
====
