---- MODULE LM5 ----
\* Contract (G-1) under VIEW: TLC evaluates the properties on the concrete
\* states of its counterexample, whose cycle closes from x = 1 back to
\* x = 0 (the real successor has x = 2): R2 alone holds, but the
\* counterexample violates it.
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
Next == y' = 1 - y /\ x' = (x + 1) % 4
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
V == y
R1 == []<>(x = 2)
R2 == <>[][x' = x + 1]_x
R3 == []<>(x = 3)
R4 == <>(x = 1)
====
