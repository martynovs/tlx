---- MODULE SW2 ----
\* Values the pre-run cannot foresee: computed strings (top level, in a
\* record, in a sequence, in a set and in a materialized definition), model
\* values and growing counters. In compiled mode with a tiny pre-run
\* (SW2.flags) each of them misses the layout: strings must widen to
\* dynamic values and integers grow fast, so that the widening converges
\* in a few restarts.
EXTENDS Naturals, Sequences, TLC
CONSTANT Procs
VARIABLES x, y, s, r, q, t, p
Name(a, b) == "n" \o ToString(a * 100 + b)
Label == "L" \o ToString(x + y)
Init == /\ x = 0 /\ y = 0 /\ s = Name(0, 0) /\ r = [name |-> "n", n |-> 0]
        /\ q = <<>> /\ t = {} /\ p = CHOOSE e \in Procs : TRUE
Step(x1, y1) == /\ x' = x1 /\ y' = y1
                /\ s' = Name(x1, y1)
                /\ r' = [name |-> ToString(x1), n |-> x1 * y1]
                /\ q' = IF Len(q) < 2 THEN Append(q, ToString(y1)) ELSE <<ToString(y1)>>
                /\ t' = {ToString(x1 % 4), ToString(y1 % 3)}
                /\ p' = IF x1 % 2 = 0 THEN p ELSE CHOOSE e \in Procs : e # p
Next == \/ x < 24 /\ Step(x + 1, y)
        \/ y < 24 /\ Step(x, y + 1)
LabelOk == Len(Label) > 1
====
