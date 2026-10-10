---- MODULE LzRecCycleNext ----
\* The cfg redirects Next to ReDefNext, which refers to Next: expanding the
\* next-state relation into actions never ends. TLC reports a
\* StackOverflowError; tlx must report an evaluation error rather than crash.
VARIABLE x
Init == x = FALSE
A(n) == x' = ~x
Next == \E i \in {1, 2} : A(i)
ReDefNext == Next /\ ~A(1)
====
