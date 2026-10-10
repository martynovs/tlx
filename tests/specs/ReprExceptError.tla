---- MODULE ReprExceptError ----
\* EXCEPT on a present key of a bag keyed by records with a sequence field
\* evaluates the replacement: its error is the run's (TLC reports it).
EXTENDS Naturals, Sequences, TLC
VARIABLES bag, n
K(i) == [id |-> i, log |-> <<i>>]
Init == bag = (K(1) :> 0) /\ n = 0
Next == \/ n = 0 /\ bag' = [bag EXCEPT ![K(2)] = 1 \div 0] /\ n' = 1
        \/ n = 1 /\ bag' = [bag EXCEPT ![K(1)] = 1 \div 0] /\ n' = 2
====
