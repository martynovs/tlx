---- MODULE TraceOp ----
\* TLCExt!Trace in the next-state relation (the path to the state expanded),
\* and an invariant (the path to the successor checked): the
\* state space depends on its value (y is the length of the path to the
\* predecessor).
EXTENDS Naturals, Sequences, TLCExt

VARIABLE x, y

Init == x = 0 /\ y = 0

Next == /\ x < 6
        /\ \/ x' = x + 1 /\ y' = Len(Trace)
           \/ x' = x + 2 /\ y' = y

Inv == /\ Trace[1] = [x |-> 0, y |-> 0]
       /\ Trace[Len(Trace)] = [x |-> x, y |-> y]
       /\ \A i \in 1..Len(Trace) - 1 : Trace[i + 1].x > Trace[i].x
       /\ y < Len(Trace)

====
