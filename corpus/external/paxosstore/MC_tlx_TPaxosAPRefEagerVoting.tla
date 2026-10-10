---- MODULE MC_tlx_TPaxosAPRefEagerVoting ----
\* tlx corpus (ours): the TLC models of PaxosStore-TLA's experiment/config/*.ini
\* (TPaxosAP refines EagerVoting) as a module and cfg files: Participant and Value are sets of model
\* values, Nat (the ballots) is overridden by 1..N, None is a model value.
EXTENDS TPaxosAPWithVotes, TLC
CONSTANTS p1, p2, v1, v2, mv_None
tlx_Participant == {p1, p2}
tlx_Value == {v1, v2}
tlx_None == mv_None
tlx_Nat2 == 1..2
tlx_Nat3 == 1..3
tlx_SymmP == Permutations(tlx_Participant)
tlx_SymmPV == Permutations(tlx_Participant) \cup Permutations(tlx_Value)
VSpec == V!Spec
====
