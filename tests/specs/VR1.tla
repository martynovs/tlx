---- MODULE VR1 ----
\* VIEW hides h, which the guards read: which state of a VIEW class is
\* found first (in BFS order) decides what is reachable. Wide levels, so
\* that several workers expand them in parallel.
EXTENDS Naturals
VARIABLES a, b, c, h
Init == a = 0 /\ b = 0 /\ c = 0 /\ h = 0
Next == \/ a < 12 /\ h # 4 /\ a' = a + 1 /\ h' = (h * 3 + b + 1) % 7 /\ UNCHANGED <<b, c>>
        \/ b < 12 /\ h # 5 /\ b' = b + 1 /\ h' = (h + a + c) % 7 /\ UNCHANGED <<a, c>>
        \/ c < 12 /\ h < 5 /\ c' = c + 1 /\ h' = (h * 2 + a) % 7 /\ UNCHANGED <<a, b>>
        \/ h = 6 /\ a' = (a + b) % 13 /\ h' = 0 /\ UNCHANGED <<b, c>>
View == <<a, b, c>>
====
