---- MODULE LF11Inner ----
VARIABLES d, c
Act == d' = FALSE /\ c' = 1
ISpec == WF_<<d, c>>(Act)
====
