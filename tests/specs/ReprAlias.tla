---- MODULE ReprAlias ----
\* Function aliases (docs/value-store.md §2.1): a
\* tuple, a record and functions built over enumerated domains (`:>`, `@@`,
\* a constructor over a set) are equal when they denote the same function.
\* States holding either are one state, they fingerprint alike, and the
\* operations with keys in the domain agree on them.
EXTENDS Naturals, Sequences, TLC
VARIABLES x, r, pc
Init == x = <<1, 2>> /\ r = [a |-> 1] /\ pc = 0
Next == \/ pc = 0 /\ x' = [i \in {1, 2} |-> i] /\ r' = ("a" :> 1) /\ pc' = 1
        \/ pc = 0 /\ x' = (1 :> 1 @@ 2 :> 2) /\ r' = [k \in {"a"} |-> 1] /\ pc' = 1
        \/ pc = 0 /\ UNCHANGED <<x, r>> /\ pc' = 1
        \/ pc = 1 /\ x' = [x EXCEPT ![1] = 3] /\ r' = [r EXCEPT !.a = 2] /\ pc' = 2
        \/ pc = 1 /\ x' = x @@ (3 :> 3) /\ r' = r @@ ("b" :> 2) /\ pc' = 3
        \/ pc = 2 /\ x' = Append(x, x[1] + r.a) /\ r' = r /\ pc' = 4
        \/ pc = 3 /\ x' = Append(x, Len(x)) /\ r' = [r EXCEPT !["b"] = @ + r.a] /\ pc' = 4
        \/ pc = 4 /\ x' = Tail(x) /\ r' = r /\ pc' = 5
Inv == /\ pc = 1 => x = <<1, 2>> /\ r = [a |-> 1] /\ DOMAIN x = 1..2 /\ r.a = 1
       /\ pc = 3 => x = <<1, 2, 3>> /\ r = [a |-> 1, b |-> 2] /\ Len(x) = 3
====
