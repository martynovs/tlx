---- MODULE LZ2 ----
\* A function built outside an application is eager: DOMAIN and EXCEPT
\* (whose `@` is evaluated) fail at the bad point.
EXTENDS Integers
VARIABLE x
Init == x = 0
Next == x' = [[i \in 1..3 |-> 10 \div (i - 2)] EXCEPT ![2] = @ + 1][1]
====
