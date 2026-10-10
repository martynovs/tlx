---- MODULE ParallelDeadlockDepth ----
EXTENDS Naturals
\* Other workers can keep expanding the long branches while the short branch
\* reports its deadlock. Multi-worker search depth is intentionally not pinned.
VARIABLES branch, step
Init == /\ branch \in 1..64
        /\ step = 0
Next == /\ step < IF branch = 1 THEN 2 ELSE 20
        /\ step' = step + 1
        /\ UNCHANGED branch
====
