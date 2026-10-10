---- MODULE SN7 ----
\* Equality of two Dyn-encoded values. The pre-run fails on Init
\* (comparing a boolean with a set is a TLC error), so every variable gets
\* the Dyn layout; compiled code compared the interned handles and
\* produced a state instead of the error.
VARIABLES b, e, r
Init == b = FALSE /\ e = {} /\ r = (b # e)
Next == UNCHANGED <<b, e, r>>
====
