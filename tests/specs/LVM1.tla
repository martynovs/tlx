---- MODULE LVM1 ----
\* A layout miss after the search. LVM1.flags forces a one-state
\* pre-run: the compiled search misses on x and q and resumes, so it
\* records no liveness edges, and the liveness phase explores again. The
\* invariant part of Prop has mq materialized ({0} in the pre-run); its
\* values with q # <<>> do not fit that shape, which only the native
\* liveness atom finds, after the search.
EXTENDS Naturals, Sequences
VARIABLES x, q
mq == {x, IF q = <<>> THEN x ELSE x + 10}
Init == x = 0 /\ q = <<>>
Next == \/ x < 4 /\ x' = x + 1 /\ q' = q
        \/ q = <<>> /\ q' = <<x>> /\ x' = x
        \/ q # <<>> /\ q' = <<>> /\ x' = x
Spec == Init /\ [][Next]_<<x, q>> /\ WF_<<x, q>>(Next)
Prop == [](x > 100 => 99 \notin mq) /\ []<>(mq = {x})
====
