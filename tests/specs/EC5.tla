---- MODULE EC5 ----
\* Contract: a module-scoped `<-` names a definition the module does not
\* have (TLC: 2226, exit 151).
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
G == 1
====
