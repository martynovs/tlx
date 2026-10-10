---- MODULE O1 ----
EXTENDS Naturals
VARIABLES s, done, b, zz, a
vars == <<s, done, b, zz, a>>
Init == s = 0 /\ done = FALSE /\ b = 1 /\ zz = 2 /\ a = 3
Toggle == s' = 1 - s /\ UNCHANGED <<done, b, zz, a>>
Spec == Init /\ [][Toggle]_vars /\ WF_vars(Toggle)
Prop == <>done
Inv == s < 1
====
