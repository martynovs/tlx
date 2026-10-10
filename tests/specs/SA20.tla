---- MODULE SA20 ----
\* TLC's `Any` (module TLC, TLC's AnySet): `v \in Any` is TRUE for every
\* value, whatever its type, but Any is not enumerable, not finite and
\* cannot be compared. This spec covers the supported cases; SA21-SA25
\* cover the errors (equality, cardinality, quantifier, subseteq, a lazy
\* `\cup` that has to be enumerated).
EXTENDS TLC, Integers, Sequences, FiniteSets
CONSTANT mv
VARIABLE x
Init == x = 0
Next == x' = x
Inv ==
  /\ x = 0
  \* membership is TRUE for every value type
  /\ 0 \in Any
  /\ -1 \in Any
  /\ "a" \in Any
  /\ {1, 2} \in Any
  /\ {} \in Any
  /\ <<1, 2>> \in Any
  /\ [a |-> 1] \in Any
  /\ Any \in Any
  /\ Nat \in Any
  /\ Int \in Any
  /\ mv \in Any
  \* `\cup` stays lazy: membership needs no enumeration
  /\ 1 \in (Any \cup {1})
  /\ 2 \in (Any \cup {1})
  \* `\cap` / `\` are driven by their enumerable side
  /\ 1 \in (Any \cap {1})
  /\ 2 \notin (Any \cap {1})
  /\ Any \cap {1} = {1}
  /\ {1} \cap Any = {1}
  /\ 1 \notin (Any \cap {})
  /\ 1 \notin (Any \ {1})
  /\ 2 \in (Any \ {1})
  \* subseteq enumerates its left side only
  /\ {1, 2} \subseteq Any
  /\ {} \subseteq Any
  /\ {1} \in SUBSET Any
  /\ {} \in SUBSET Any
  \* not finite; prints as ANY
  /\ ~IsFiniteSet(Any)
  /\ ToString(Any) = "ANY"
  /\ PrintT(Any) \in Any
====
