---- MODULE LS3 ----
\* Run-time strings are ordered by creation, so the
\* compiled tier's faithful pre-run must create them in the search's order.
\* The search evaluates the liveness atom (x = "a" \o "z") on every state it
\* expands and so creates "az" before Next creates "abb" (TLC, interpreter);
\* a pre-run that did not evaluate the atoms created them the other way
\* round, the CHOOSE picked "abb" and Inv failed (TLC: no error).
EXTENDS Naturals, Sequences
VARIABLES x, n
vars == <<x, n>>
Init == x = "a" /\ n = 0
Next == \/ /\ n < 6
           /\ x' = IF n = 3 THEN "a" \o "z" ELSE x \o "b"
           /\ n' = IF n = 4 /\ (CHOOSE s \in {"a" \o "z", ("a" \o "b") \o "b"} : TRUE) = "a" \o "z" THEN 5
                   ELSE IF n = 4 THEN 7 ELSE n + 1
        \/ n >= 6 /\ UNCHANGED vars
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
Prop == <>(x = "a" \o "z")
Inv == n # 7
====
