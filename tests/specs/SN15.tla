---- MODULE SN15 ----
\* Fuzzer finding (seed 23): DOMAIN of an encoded function, tuple or record
\* has a domain fixed by its shape, but the operand must still be
\* evaluated: TLC reports the division by zero in the EXCEPT value. The
\* compiled tier returned the domain without evaluating it.
EXTENDS Integers, TLC
CONSTANT m3
VARIABLES n, f, t
Init == n = 0 /\ f = (m3 :> 0) /\ t = <<1, 2>>
Next == /\ n < 2
        /\ n' = n + 1
        /\ UNCHANGED <<f, t>>
Inv == /\ DOMAIN [t EXCEPT ![1] = @ + 1] = 1..2
       /\ (n = 1 => DOMAIN [f EXCEPT ![m3] = (@ \div 0)] = {m3})
====
