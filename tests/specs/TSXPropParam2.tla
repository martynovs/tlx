---- MODULE TSXPropParam2 ----
\* As TSXPropParam with top-level operators: the property is named after
\* the parameter def_arg of x1 (TLC's LetDef2BoxedbTest).
VARIABLE x
vars == x
x0 == x
Init == x = TRUE
Next == x' = ~x
Spec == Init /\ [][Next]_vars
x2(def_arg) == def_arg
x1(def_arg(_)) == def_arg([][x']_x0)
Prop2 == x1(x2)
====
