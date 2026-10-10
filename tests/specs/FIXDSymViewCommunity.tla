---- MODULE FIXDSymViewCommunity ----
\* As FIXDSymViewOrder, with CommunityModules extended: TLC's order of the
\* state values (by the hash codes of the variable names in the root module's
\* symbol table) depends on the size of that table, which counts the symbols
\* of the extended CommunityModules too.
EXTENDS Naturals, TLC, SequencesExt, FiniteSetsExt, Functions, Folds
CONSTANTS p1, p2
VARIABLES alpha, beta, gamma, delta, eps
Procs == {p1, p2}
Init == /\ alpha \in [Procs -> 0..1]
        /\ beta \in [Procs -> 0..1]
        /\ gamma \in [Procs -> 0..1]
        /\ delta \in [Procs -> 0..1]
        /\ eps = 0
Next == \E p \in Procs :
          /\ eps < 2
          /\ alpha' = [alpha EXCEPT ![p] = 1 - @]
          /\ beta' = [beta EXCEPT ![p] = 1 - @]
          /\ eps' = eps + 1
          /\ UNCHANGED <<gamma, delta>>
View == <<alpha, beta, gamma, delta>>
ViewA == <<alpha, eps>>
ViewB == beta
ViewC == gamma
ViewD == delta
Sym == Permutations(Procs)
====
