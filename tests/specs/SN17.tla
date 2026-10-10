---- MODULE SN17 ----
\* Membership decided by the layout of the element alone (a tuple
\* of another length is in no product or function set) must still evaluate
\* the element: here the EXCEPT value does not fit the field's layout
\* (a runtime layout miss in the compiled tier).
EXTENDS Naturals, Sequences
VARIABLES v, u
Init == /\ v = [a |-> <<1>>, c |-> 2]
        /\ u \in {<<>>, <<3>>, <<4, 5>>}
Next == UNCHANGED <<v, u>>
Product == /\ (([v EXCEPT !.a = u]).a \in Nat \X Nat) = (Len(u) = 2)
           /\ ([v EXCEPT !.a = <<1, 2, 3>>]).a \in Nat \X Nat \X Nat
           /\ ~(([v EXCEPT !.a = <<1, 2, 3>>]).a \in Nat \X Nat)
FcnSet == /\ (([v EXCEPT !.a = u]).a \in [1..2 -> Nat]) = (Len(u) = 2)
          /\ ([v EXCEPT !.a = <<1, 2, 3>>]).a \in [1..3 -> Nat]
RecSet == /\ [v EXCEPT !.a = u] \in [a : Seq(Nat), c : Nat]
          /\ ([v EXCEPT !.a = u]).a \in {<<>>, <<3>>, <<4, 5>>}
          /\ ([v EXCEPT !.a = u]).a \in Seq(Nat)
Eq == /\ ([v EXCEPT !.a = u]).a = u
      /\ [v EXCEPT !.a = <<1, 2, 3>>] = [a |-> <<1, 2, 3>>, c |-> 2]
====
