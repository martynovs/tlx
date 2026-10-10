---- MODULE Y1 ----
\* SYMMETRY over a function with a varying domain (a Map encoding), a set,
\* a sequence of records and a function over the symmetric set.
EXTENDS Naturals, Sequences, FiniteSets, TLC
CONSTANTS P
VARIABLES owner, held, log, mode
Init == /\ owner = [p \in {} |-> 0]
        /\ held = {}
        /\ log = <<>>
        /\ mode = [p \in P |-> "idle"]
Add(p) == /\ p \notin DOMAIN owner
          /\ owner' = owner @@ (p :> Cardinality(DOMAIN owner))
          /\ held' = held \cup {p}
          /\ UNCHANGED <<log, mode>>
Drop(p) == /\ p \in DOMAIN owner
           /\ owner' = [q \in DOMAIN owner \ {p} |-> owner[q]]
           /\ held' = held \ {p}
           /\ mode' = [mode EXCEPT ![p] = "done"]
           /\ UNCHANGED log
Bump(p) == /\ p \in DOMAIN owner
           /\ owner[p] < 2
           /\ owner' = [owner EXCEPT ![p] = @ + 1]
           /\ UNCHANGED <<held, log, mode>>
Log(p) == /\ Len(log) < 2
          /\ log' = Append(log, [who |-> p, n |-> Cardinality(held)])
          /\ UNCHANGED <<owner, held, mode>>
Next == \E p \in P : Add(p) \/ Drop(p) \/ Bump(p) \/ Log(p)
Sym == Permutations(P)
Inv == DOMAIN owner = held
====
