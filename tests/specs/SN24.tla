---- MODULE SN24 ----
\* i \in DOMAIN g with a domain fixed by the shape of g is a range test,
\* but g is still evaluated: TLC reports the division by zero in the
\* EXCEPT value (SN15: DOMAIN compared with a set).
EXTENDS Integers
VARIABLES n, t
Init == n = 0 /\ t = <<1, 2>>
Next == /\ n < 2
        /\ n' = n + 1
        /\ t' = t
Inv == /\ 1 \in DOMAIN [t EXCEPT ![1] = @ + 1]
       /\ (n = 1 => 2 \in DOMAIN [t EXCEPT ![1] = @ \div 0])
====
