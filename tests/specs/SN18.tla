---- MODULE SN18 ----
\* Len of a fixed-length tuple evaluates its components (TLC
\* reports the out-of-range index; the compiled tier folded the length).
EXTENDS Naturals, Sequences
VARIABLES w
Init == w = <<4, 5>>
Next == w' = <<3>>
Inv == Len(<<w[2]>>) = 1
====
