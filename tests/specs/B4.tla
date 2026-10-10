---- MODULE B4 ----
EXTENDS Naturals, Sequences
VARIABLE x, q
Init == x = 1 /\ q = <<1, 2, 3>>
Next == /\ x' = (x % 3) + 1
        /\ q' = SelectSeq(<<1, 2, 3>>, LAMBDA e : e # x)
====
