---- MODULE FZ4 ----
\* Found by scripts/fuzz_expr.py (compiled tier): a constant branch that does
\* not fit the other branch's encoding (here the fixed-length shape of
\* `v = <<>>`) must not be converted into it: Len/DOMAIN of the IF only look
\* at the shape and returned 0 / {}.
EXTENDS Integers, Sequences
VARIABLES v, r, d, pc
Init == v = <<>> /\ r = 0 /\ d = {} /\ pc = 0
Next == /\ pc = 0
        /\ pc' = 1
        /\ r' = Len(IF r = 1 THEN v ELSE <<1>>)
        /\ d' = DOMAIN (IF r = 1 THEN v ELSE <<1, 2>>)
        /\ v' = v
Inv == pc = 1 => r = 1 /\ d = 1..2
====
