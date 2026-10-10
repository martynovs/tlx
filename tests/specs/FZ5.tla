---- MODULE FZ5 ----
\* Found by scripts/fuzz_expr.py: operators TLC overrides by Java methods
\* evaluate their arguments (here Bags' (-), whose TLA+ definition does not
\* use B2 when B1 is empty): an error in the argument is reported.
EXTENDS Integers, Bags
ASSUME EmptyBag (-) (CHOOSE n \in {} : TRUE) = EmptyBag
VARIABLE x
Init == x = 0
Next == UNCHANGED x
====
