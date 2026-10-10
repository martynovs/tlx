---- MODULE LF8 ----
\* Matrix L145: an error in Next at x = 2 with fairness of Next.
\* TLC still checks liveness on that state, where ENABLED <<Next>>_v fails
\* too: exit 255 instead of 75.
EXTENDS Naturals, Sequences
VARIABLES x, log
Init == x = 0 /\ log = <<>>
Push(t) == /\ x' = x + 1
           /\ log' = Append(log, t)
           /\ (x = 2 => log[3] \in {"a"})
Next == \E t \in {"a"} : Push(t)
Spec == Init /\ [][Next]_<<x, log>> /\ WF_<<x, log>>(Next)
Prop == []<>(x = 0)
====
