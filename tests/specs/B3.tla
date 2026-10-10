---- MODULE B3 ----
EXTENDS Naturals
VARIABLE x
Init == x = <<0, 0>>
Next == x' = [x EXCEPT ![5] = 1]
====
