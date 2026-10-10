---- MODULE V2 ----
\* VIEW computed from the state (a record of derived values): states with
\* the same VIEW are one.
EXTENDS Naturals, FiniteSets
VARIABLES x, y, s
Init == x = 0 /\ y = 0 /\ s = {}
Next == \/ x < 6 /\ x' = x + 1 /\ UNCHANGED <<y, s>>
        \/ y < 4 /\ y' = y + 1 /\ UNCHANGED <<x, s>>
        \/ Cardinality(s) < 3 /\ s' = s \cup {x + y} /\ UNCHANGED <<x, y>>
View == [par |-> x % 2, sum |-> x + y, rem |-> {e % 3 : e \in s}]
====
