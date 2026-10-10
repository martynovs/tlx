---- MODULE G2 ----
\* Generation (-generate) does not support CASE in actions (TLC: an
\* UnsupportedOperationException).
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == CASE x < 2 -> x' = x + 1
          [] OTHER -> x' = 0
====
