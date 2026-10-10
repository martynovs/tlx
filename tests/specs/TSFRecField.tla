---- MODULE TSFRecField ----
\* Contract: `Nat`, `Int` and `Real` are ordinary names, so they can be
\* record fields (`[Nat |-> 1]`, `[Nat : S]`, `r.Nat`; SANY only lints them)
\* while `\in Nat` keeps meaning the set (Vanlightly's VSR.tla writes
\* `log: [Nat |-> LogEntryType]`).
EXTENDS Integers
VARIABLE r
ASSUME [Nat |-> 1].Nat = 1 /\ DOMAIN [Int |-> 2, Real |-> 3] = {"Int", "Real"}
ASSUME [a : {1}, Nat : {2, 3}] = {[a |-> 1, Nat |-> 2], [a |-> 1, Nat |-> 3]}
ASSUME [n \in 1..2 |-> n] = <<1, 2>> /\ 3 \in Nat /\ -1 \notin Nat
Init == r = [Nat |-> 0, Int |-> -1]
Next == /\ r.Nat < 3
        /\ r' = [r EXCEPT !.Nat = @ + 1, !.Int = r.Nat - r . Int]
Inv == r \in [Nat : 0..3, Int : Int] /\ r.Int # 5
====
