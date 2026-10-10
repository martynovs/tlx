---- MODULE RF1 ----
\* Recursive function definitions (module level and in LETs, linear and
\* non-linear recursion, deeper than any unrolling, capturing operator
\* parameters, a LAMBDA and its captured locals).
EXTENDS Naturals, Sequences

VARIABLES x, s

Fact[n \in Nat] == IF n = 0 THEN 1 ELSE n * Fact[n - 1]

Fib[n \in Nat] == IF n < 2 THEN n ELSE Fib[n - 1] + Fib[n - 2]

SumSeq(q) ==
  LET F[i \in 0..Len(q)] == IF i = 0 THEN 0 ELSE q[i] + F[i - 1]
  IN  F[Len(q)]

MapSeq(q, Op(_)) ==
  LET G[i \in 0..Len(q)] == IF i = 0 THEN << >> ELSE Append(G[i - 1], Op(q[i]))
  IN  G[Len(q)]

SumSet(S) ==
  LET H[T \in SUBSET S] ==
        IF T = {} THEN 0
        ELSE LET e == CHOOSE e \in T : TRUE IN e + H[T \ {e}]
  IN  H[S]

Init == x = 0 /\ s = << >>

Grow == /\ x < 12
        /\ x' = x + 1
        /\ s' = IF Len(s) < 5 THEN Append(s, x % 3) ELSE s

Mix == /\ Len(s) > 0
       /\ \E k \in 1..2 :
            /\ x' = (Fib[x] + Fact[x % 9] + k) % 13
            /\ s' = MapSeq(Tail(s), LAMBDA v : (v + k + SumSeq(s)) % 3)

Next == Grow \/ Mix

Inv == /\ SumSeq(s) <= 2 * Len(s)
       /\ SumSet({v + x : v \in {1, 2, 3}}) = 6 + 3 * x
       /\ Fact[x] >= x
       /\ x < 6 \/ Fib[x] > x
====
