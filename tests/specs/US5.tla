---- MODULE US5 ----
\* Strings created by an invariant's subformula: TLC evaluates Foo
\* only where the disjunction gets to it (n >= 2), as it checks each new
\* state, so "850" and "849" are first created by the action at n = 3, after
\* the strings of the earlier steps. A checker that evaluates Foo in every
\* state ahead of the search (e.g. to infer shapes) would create them first.
EXTENDS Naturals, TLC
VARIABLES n, s

Foo == ToString(851 - (n + 1))
Foo2(k) == ToString(851 - (k + 1))

Init == n = 0 /\ s = {}

Next ==
    /\ n < 4
    /\ \E d \in 1..2 :
         /\ n' = n + 1
         /\ s' = s \cup {ToString(901 - (n * 10 + d))} \cup (IF n = 3 THEN {Foo2(k) : k \in 0..3} ELSE {})

Inv == n < 2 \/ Foo /= "x"
Done == n < 4
====
