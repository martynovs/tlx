---- MODULE LZT1 ----
\* A predicate of the tableau fails at (x = 4, b = 0); TLC evaluates
\* it when it expands (x = 3, b = 0), before it reaches the invariant
\* violated at (x = 4, b = 1). The search finds the invariant violation
\* first, the predicate never evaluated on (x = 4, b = 0) when it expands
\* states: it is evaluated on every new state found.
EXTENDS Naturals
VARIABLES x, b
vars == <<x, b>>
Init == x = 0 /\ b \in {0, 1}
Next == x < 4 /\ x' = x + 1 /\ b' = b
Spec == Init /\ [][Next]_vars
Inv == ~(b = 1 /\ x = 4)
Prop == (x = 1) ~> (10 \div (4 - x) > 0)
====
