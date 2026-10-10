---- MODULE SN8 ----
\* Equality of two values of one union shape (integers and sets)
\* compared their encodings: 2 # {} was FALSE instead of TLC's error. The
\* small pre-run (SN8.flags) sees integers only; the layout is widened to
\* the union when y becomes a set.
EXTENDS Naturals
VARIABLES n, x, y
Init == n = 0 /\ x = 0 /\ y = 0
Next == /\ n < 5
        /\ n' = n + 1
        /\ x' = IF n = 3 THEN {} ELSE n
        /\ y' = IF n = 2 THEN {} ELSE n
Inv == x # y \/ n < 3
====
