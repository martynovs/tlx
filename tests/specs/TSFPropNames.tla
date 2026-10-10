---- MODULE TSFPropNames ----
\* Contract: the name TLC reports for a PROPERTY part checked during the
\* search (SpecProcessor.processConfigProps): a conjunct is named by its
\* location, so the action property violated here is
\* "line 13, col 20 to line 13, col 31 of module TSFPropNames".
EXTENDS Naturals
VARIABLE s
Init == s = 0
Next == s' = s + 1
Spec == Init /\ [][Next]_s
Bound == s < 20
Alias == Bound
Prop == []Alias /\ [][s' # 4]_s /\ [](s # 6)
====
