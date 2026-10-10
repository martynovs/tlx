---- MODULE FZ2 ----
\* Found by scripts/fuzz_expr.py: TLC passes operator arguments unevaluated,
\* so an argument the body does not use cannot fail (also in invariants).
\* (Not for operators TLC overrides by Java methods: Bags' (-) evaluates B.)
EXTENDS Integers, Bags
G(a) == 1
Restrict(f, S) == [x \in S |-> f[x]]
ASSUME G(CHOOSE n \in {} : TRUE) = 1
ASSUME (LET H(a) == {} IN H(CHOOSE n \in {} : TRUE)) = {}
ASSUME Restrict([{1} EXCEPT ![1] = 3], {}) = <<>>
VARIABLE x
Init == x \in 0..2
Next == x' = (x + 1) % 3
Inv == G(x \div 0) = 1 /\ Restrict([{x} EXCEPT ![1] = 3], {}) = <<>>
====
