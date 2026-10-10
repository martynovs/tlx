---- MODULE UnionAssignmentComparisonError ----
EXTENDS Naturals
VARIABLES s, t
Init == \/ s = {1} /\ t = {"a"}
        \/ s = {"a"} /\ t = {1}
Next == s' = s \cup t /\ UNCHANGED t
====
