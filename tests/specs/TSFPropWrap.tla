---- MODULE TSFPropWrap ----
\* Contract: TLC classifies a PROPERTY through LET and operators applied to
\* arguments (SpecProcessor.processConfigProps binds them): `[]P` parts are
\* checked as invariants during the search, `[][A]_v` parts as action
\* properties. A part is named by the innermost definition it was reached
\* through (`Q`), by `Inv` in `[]Inv`, or by the location of a conjunct; so
\* the violation found first here is reported as `Q` (TLC tests Github817c,
\* LetDef1Boxedb, Github1161Violated).
EXTENDS Naturals
VARIABLE s
Init == s = 0
Next == s' = s + 1
Spec == Init /\ [][Next]_s
Inv == s # 9
Q(a) == [](s # a)
Steps == LET Small(b) == [][b]_s IN Small(s' < 8)
Prop == Q(5) /\ []Inv /\ [](s # 7) /\ Steps
====
