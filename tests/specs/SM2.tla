---- MODULE SM2 ----
\* The empty function of an enumerated domain ([x \in {} |-> e], Restrict,
\* EmptyBag, ...) equals <<>>, but EXCEPT leaves it unchanged for any key,
\* where <<>> rejects a non-integer key (TLC: FcnRcdValue vs TupleValue).
\* Also kept in state variables and compared with <<>>.
EXTENDS Naturals, Sequences, Bags, TLC, Functions
CONSTANT K
VARIABLES f, s, n
E == [x \in {} |-> 0]
Init == /\ f = E
        /\ s = <<>>
        /\ n = 0
Next == /\ n < 3
        /\ n' = n + 1
        /\ f' = [f EXCEPT ![K] = n, !["a"] = 1]
        /\ s' = IF n = 1 THEN [Restrict(<<1>>, {}) EXCEPT ![K] = 2] ELSE Append(s, n)
Inv == /\ f = <<>>
       /\ f = E
       /\ DOMAIN f = {}
       /\ Len(f) = 0
       /\ ToString(f) = "<<>>"
       /\ [EmptyBag EXCEPT ![K] = 1] = <<>>
       /\ [[x \in {} |-> 1] @@ <<>> EXCEPT !["b"] = 1] = <<>>
       /\ [<<E>> EXCEPT ![1][K] = 2] = <<<<>>>>
       /\ {f, <<>>, EmptyBag} = {<<>>}
       /\ s \in Seq(Nat)
====
