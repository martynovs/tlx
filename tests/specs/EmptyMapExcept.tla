---- MODULE EmptyMapExcept ----
EXTENDS Naturals, TLC
VARIABLE rec
Init == rec = ("m" :> 0)
Drop == rec' = <<>>
Update == rec' = [rec EXCEPT !["m"] = 2]
Next == Drop \/ Update
====
