---- MODULE TSXPropParam ----
\* An action property reached through an operator parameter: TLC names
\* the violated property by the operator as written in the application
\* (the parameter x2), not by the operator passed for it (x3)
\* (SpecProcessor.processConfigProps; TLC's LetDef2BoxedTest).
VARIABLE x
vars == x
x0 == x
Init == x = TRUE
Next == x' = ~x
Spec == Init /\ [][Next]_vars
x3(def_arg) == def_arg
Prop ==
    LET x1(x2(_)) == x2([][x']_x0)
    IN x1(x3)
====
