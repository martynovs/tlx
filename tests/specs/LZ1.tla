---- MODULE LZ1 ----
\* Lazy function constructors (TLC's FcnLambdaValue and KeepLazy): the
\* function of an application `f[e]` is not built, so errors at points that
\* are not applied do not occur.
EXTENDS Integers, Sequences
VARIABLES x, y
F(n) == [i \in 1..n |-> 10 \div (i - 2)]
\* A constant definition whose (eager) value fails: TLC re-evaluates it
\* lazily where it is applied.
G == [i \in 0..3 |-> 10 \div i]
\* A state-level definition with an error at an unused point.
H == [i \in 0..3 |-> (x + 10) \div i]
\* The body of a recursive function is evaluated with KeepLazy.
T[n \in 0..3] == IF n = 0 THEN [i \in 0..2 |-> 100 \div i]
                 ELSE [i \in 0..2 |-> T[n - 1][i] + 1]
K(k) == [i \in 1..k |-> IF i = k THEN CHOOSE z \in {} : TRUE ELSE i]
ASSUME [i \in 1..3 |-> 10 \div (i - 2)][1] = -10
ASSUME F(3)[3] = 10
ASSUME (IF TRUE THEN [i \in 1..3 |-> 10 \div (i - 2)] ELSE <<>>)[1] = -10
ASSUME (LET h == [i \in 1..3 |-> 10 \div (i - 2)] IN h[1]) = -10
ASSUME [i \in 1..3 |-> [j \in 1..3 |-> 10 \div (j - 2)]][1][3] = 10
Init == x = 0 /\ y = G[2]
Next == /\ x < 3
        /\ x' = x + H[1] - 9 + T[3][1] - 103 + K(4)[2] - 2
        /\ y' = [i \in 1..2 |-> [j \in 1..2 |-> i * j]][2]
Inv == G[1] = 10 /\ H[2] >= 5 /\ (LET r == [i \in 0..1 |-> 1 \div i] IN r[1]) = 1
====
