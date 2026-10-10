---- MODULE C1 ----
EXTENDS TLC, Sequences
VARIABLE x
Init == x = CHOOSE s \in {"q", "f"} : TRUE
Next == UNCHANGED x
Inv == x = "q"
====
