---- MODULE EC3 ----
\* Contract: a `<-` names a module the spec does not have (TLC: 2245, exit 151).
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
G == 1
====
