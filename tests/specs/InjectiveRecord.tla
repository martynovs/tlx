---- MODULE InjectiveRecord ----
EXTENDS Integers, TLC, Functions
VARIABLE x
Init == x = 0
Next == x' = 1 - x
Inv == /\ IsInjective([b |-> x, c |-> x + 1])
       /\ ~IsInjective([b |-> x, c |-> x])
====
