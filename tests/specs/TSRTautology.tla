---- MODULE TSRTautology ----
\* A temporal property whose negation folds to FALSE is a tautology: TLC
\* refuses to check it (EC.TLC_LIVE_FORMULA_TAUTOLOGY).
VARIABLES x
Init == x = 0
Next == x' = 1
Spec == Init /\ [][Next]_x
Prop == <>[]TRUE => TRUE
====
