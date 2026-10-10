---- MODULE FZ3 ----
\* Found by scripts/fuzz_expr.py: TLC prints a function over strings as a
\* record only if every string is a name (FcnRcdValue.isRcd/isName).
EXTENDS Integers, TLC
ASSUME ToString([s \in {"", "c"} |-> 1]) = "(\"\" :> 1 @@ \"c\" :> 1)"
ASSUME ToString("a b" :> 1) = "(\"a b\" :> 1)"
ASSUME ToString("_" :> 1) = "(\"_\" :> 1)"
ASSUME ToString("1a" :> 1) = "[1a |-> 1]"
ASSUME ToString("WF_x" :> 1) = "(\"WF_x\" :> 1)"
ASSUME ToString([a |-> 1, b2 |-> 2]) = "[a |-> 1, b2 |-> 2]"
VARIABLE x
Init == x = 0
Next == UNCHANGED x
====
