---- MODULE US3 ----
\* Order of strings created during the search: TLC interns a string
\* when it is first created, so the order of run-time strings follows the
\* breadth-first exploration (one worker), the order of the successors of a
\* state, and the invariant checks of each new state as it is generated
\* (Inv creates "400", "399", ... one level ahead of the action that adds
\* them to s). Lab counts down so that the creation order differs from the
\* numeric and the lexicographic order.
EXTENDS Naturals, TLC
VARIABLES n, s, c

Lab(k) == ToString(1000 - k)

Init == n = 0 /\ s = {} /\ c = "none"

Next ==
    /\ n < 3
    /\ \E d \in 1..2 :
         /\ n' = n + 1
         /\ s' = s \cup {Lab(n * 10 + d)} \cup (IF n = 2 THEN {Lab(600 + m) : m \in 0..3} ELSE {})
         /\ c' = CHOOSE e \in s' : TRUE

Inv == Lab(600 + n) /= "x"
Done == n < 3
====
