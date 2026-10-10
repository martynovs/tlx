---- MODULE L6 ----
EXTENDS Naturals
VARIABLES s, done
Init == s = 0 /\ done = FALSE
Toggle == s' = 1 - s /\ UNCHANGED done
Finish == s = 1 /\ done' = TRUE /\ UNCHANGED s
Spec == Init /\ [][Toggle \/ Finish]_<<s, done>> /\ WF_<<s, done>>(Toggle) /\ SF_<<s, done>>(Finish)
Prop == <>done
====
