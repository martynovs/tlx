---- MODULE TSFSpecTrivial ----
\* Contract: a conjunct of the SPECIFICATION naming a constant-level
\* definition that is FALSE is an error (TLC_CONFIG_SPEC_IS_TRIVIAL), not a
\* spec without initial states (TLC test Github687specD).
EXTENDS Naturals
VARIABLE x
Sane == 2 + 2 = 5
Init == x = 0
Next == x' = x
Spec == Init /\ [][Next]_x /\ Sane
====
