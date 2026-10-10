---- MODULE SN19 ----
\* Membership decided by the element's layout alone (a record
\* without the set's fields) still evaluates the element: TLC reports the
\* out-of-range index.
EXTENDS Naturals, Sequences
VARIABLES w
Init == w = <<4, 5>>
Next == w' = <<3>>
Inv == ~([x |-> w[2]] \in [y : Nat])
====
