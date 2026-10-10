---- MODULE CG3 ----
\* Recursive operators that do not compile to functions (LET
\* definitions) are unrolled a few levels in compiled code; deeper calls,
\* and the self-calls of a non-linear recursion, are evaluated by the
\* interpreter instead of failing at run time.
EXTENDS Integers
VARIABLE x, y
Init == x = 0 /\ y = 0
Next == /\ x < 12
        /\ x' = x + 1
        /\ y' = LET RECURSIVE Fib(_)
                    Fib(n) == IF n < 2 THEN n ELSE Fib(n - 1) + Fib(n - 2)
                IN Fib(x' % 10)
Inv == LET RECURSIVE F(_)
           F(n) == IF n = 0 THEN 0 ELSE 1 + F(n - 1)
       IN F(x) = x
Inv2 == LET RECURSIVE G(_, _)
            G(n, acc) == IF n = 0 THEN acc ELSE G(n - 1, acc \cup {n})
        IN G(x, {}) = 1..x
Inv3 == y <= 34
====
