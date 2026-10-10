---- MODULE TSFPossible ----
\* Contract: `_POSSIBLE P ...` (TLC's SpecProcessor.processConfigPossible):
\* each state-level P must hold in some state that satisfies the
\* CONSTRAINTs, each action-level P in some step that satisfies the
\* ACTION_CONSTRAINTs; checked after the search, before the POSTCONDITIONs,
\* the first one never witnessed fails like a POSTCONDITION, named by it.
\* Here Wrap is never evaluated (the step 3 -> 4 leaves the CONSTRAINT, and
\* the ACTION_CONSTRAINTs are evaluated only when the CONSTRAINTs hold),
\* AtFour is cut off by the CONSTRAINT and Jump never happens.
EXTENDS Naturals, TLC
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 6
Small == x < 4
AtOne == x = 1
Wrap == x = 3 /\ x' = 4
AtFour == x = 4
Jump == x' = x + 2
Post == TLCGet("distinct") = 0
====
