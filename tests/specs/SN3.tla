---- MODULE SN3 ----
\* A function TLC builds as an FcnRcdValue over an enumerated domain
\* ([k \in {1, 2} |-> e], :>, @@, bags, [S -> T] over a set) whose domain
\* is 1..n or a set of strings equals the tuple / record with the same
\* pairs, but EXCEPT looks keys up by equality: a model value is not in its
\* domain and the function is returned unchanged, where a tuple or record
\* (or an interval-domain function) rejects the key. Kept in state
\* variables (all tiers), updated by EXCEPT (still enumerated-domain), and
\* mixed with equal tuples / records (one state, not two).
EXTENDS Naturals, Sequences, Bags, TLC
CONSTANTS M
VARIABLES n, f, r, b, g, w, q
Init == /\ n = 0
        /\ f = [k \in {1, 2} |-> 0]
        /\ r = [k \in {"a", "b"} |-> 0]
        /\ b = SetToBag({1, 2})
        /\ g = (1 :> 0) @@ (2 :> 1)
        /\ w = <<0, 0>>
        /\ q = [a |-> 0, b |-> 0]
Next == /\ n < 3
        /\ n' = n + 1
        /\ f' = [f EXCEPT ![M] = 9, ![1] = n]
        /\ r' = [r EXCEPT ![M] = 9, !.a = n, !["c"] = 9]
        /\ b' = [b EXCEPT ![M] = 9]
        /\ g' = [g EXCEPT ![M] = 9, ![2] = @ + 1]
        /\ \/ w' = <<0, 0>>
           \/ w' = [k \in {1, 2} |-> 0]
           \/ w' = [x \in {1, 2} |-> 0] @@ <<1>>
        /\ \/ q' = [k \in {"a", "b"} |-> 0]
           \/ q' = [a |-> 0, b |-> 0]
           \/ q' = [q EXCEPT !.a = 0]
Inv == /\ f = <<IF n = 0 THEN 0 ELSE n - 1, 0>>
       /\ f \in Seq(Nat) /\ Len(f) = 2 /\ DOMAIN f = {1, 2}
       /\ ToString(f) = ToString(<<f[1], 0>>)
       /\ r = [a |-> IF n = 0 THEN 0 ELSE n - 1, b |-> 0]
       /\ r.a = r["a"] /\ [r EXCEPT ![M] = 5].b = 0
       /\ b = <<1, 1>> /\ BagToSet(b) = {1, 2}
       /\ g = <<0, 1 + n>>
       /\ w = <<0, 0>> /\ w[2] = 0
       /\ q = [a |-> 0, b |-> 0] /\ q.b = 0
       /\ {f, <<0, 0>>, [k \in {1, 2} |-> 0]} \subseteq Seq(Nat)
       /\ [[k \in {1, 2} |-> 0] EXCEPT ![M] = 1] = <<0, 0>>
       /\ [(CHOOSE h \in [{1, 2} -> {0}] : TRUE) EXCEPT ![M] = 1] = <<0, 0>>
====
