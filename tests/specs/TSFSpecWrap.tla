---- MODULE TSFSpecWrap ----
\* Contract: TLC splits a SPECIFICATION into Init, [][Next]_v and fairness
\* through LET and operators applied to arguments (SpecProcessor
\* processConfigSpec binds the arguments): here an identity in a LET, an
\* operator wrapping [][Next]_vars, and a parameterized instance's Spec
\* (TLC tests Github1244b, LetDef1Boxedc, Github687specA, suite test61).
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
---- MODULE Counter ----
VARIABLE c
CInit == c = 0
CNext == c' = (c + 1) % 3
CSpec == CInit /\ [][CNext]_c /\ WF_c(CNext)
====
C(c) == INSTANCE Counter
MyNext(n) == n
Init == x = 0
Next == x' = (x + 1) % 4 /\ C(y)!CNext
Spec == LET id(a) == a IN id(Init /\ C(y)!CInit) /\ MyNext([][Next]_vars) /\ WF_vars(Next)
Live == <>(x = 3) /\ []<>(y = 2)
====
