---- MODULE US4 ----
\* Strings created by state-level definitions: Lab is evaluated
\* only where the action uses it (n = 2), and Inv only on the states the
\* search generates, so their strings are created in the search's order.
\* A checker that evaluates such definitions ahead of the search (e.g. to
\* infer shapes) must not create strings early.
EXTENDS Naturals, Sequences, TLC
VARIABLES n, s, t

Lab == ToString(700 - n)
Tag(k) == "t" \o ToString(k + 5)

Init == n = 0 /\ s = {} /\ t = Tag(0)

Next ==
    /\ n < 4
    /\ \E d \in 1..2 :
         /\ n' = n + 1
         /\ s' = s \cup (IF n >= 2 THEN {Lab, ToString(700 - n - d), ToString(500 - d)} ELSE {ToString(600 - d)})
         /\ t' = Tag(n * 3 + d)

Inv == n < 4 \/ s = {}
====
