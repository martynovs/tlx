---- MODULE FIXDSymViewOrder ----
\* SYMMETRY with a VIEW that is not symmetric: TLC fingerprints the VIEW of
\* the least state of the orbit, comparing the variables in the order of its
\* state values (the order it prints them: here done before s, not the
\* declaration order), so which permutation is chosen decides the counts.
EXTENDS Naturals, TLC
CONSTANTS p1, p2
VARIABLES s, done
Procs == {p1, p2}
Init == s \in [Procs -> 0..1] /\ done \in [Procs -> 0..1]
Next == \E p \in Procs :
          /\ s[p] < 2
          /\ s' = [s EXCEPT ![p] = @ + 1]
          /\ done' = [done EXCEPT ![p] = 1 - @]
View == s
Sym == Permutations(Procs)
====
