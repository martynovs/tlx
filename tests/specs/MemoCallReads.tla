---- MODULE MemoCallReads ----
\* A RECURSIVE operator compiled to a function that is not native (SUBSET
\* in its body) is called from native code through a host region whose
\* result is memoized. The region's own expression reads no state variable:
\* only the called function's body reads x, so the memo key must include x,
\* or the fixpoint of the first state is reused for every other state (the
\* bug behind Cassandra Accord's NoStuck in compiled mixed mode).
EXTENDS Integers, FiniteSets
VARIABLES x
Step(S) == S \cup {i \in 1..3 : i \notin S /\ x[i] < Cardinality(SUBSET S)}
RECURSIVE Grow(_)
Grow(S) == LET nxt == Step(S) IN IF nxt = S THEN S ELSE Grow(nxt)
Init == x \in [1..3 -> 0..2]
Next == /\ Grow({}) # {}
        /\ x' \in {[x EXCEPT ![i] = (x[i] + 1) % 3] : i \in 1..3}
Inv == Grow({}) = Step(Step(Step({})))
====
