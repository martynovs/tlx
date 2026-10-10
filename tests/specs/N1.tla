---- MODULE N1 ----
\* Intern order (TLC: strings and model values are ordered by first
\* appearance: cfg, then the modules as parsed). The expected values are
\* TLC's choices; ToString exposes the enumeration order of sets.
EXTENDS Naturals, TLC, IBase
CONSTANTS A, B, Procs
VARIABLE x
I == INSTANCE IInst WITH C <- "cherry"
Local == {"fig", "kiwi", "banana"}
S2 == {"beta", "zeta", "gamma"}
Init == x = 0
Next == UNCHANGED x
Inv == /\ ToString(BaseS) = "{\"kiwi\", \"apple\"}"
       /\ ToString(I!InstS) = "{\"plum\", \"fig\", \"cherry\"}"
       /\ (CHOOSE s \in Local : TRUE) = "kiwi"
       /\ ToString(DOMAIN BaseR) = "{\"b\", \"k\"}"
       /\ (CHOOSE v \in {"y", "cherry", "k"} : TRUE) = "k"
       /\ (CHOOSE s \in S2 : TRUE) = "beta"
       /\ (CHOOSE m \in {A, B} : TRUE) = B
       /\ ToString(Procs) = "{p3, p1, p2, \"s\"}"
====
