---- MODULE NeonCommunityOps ----
\* Functions / FiniteSetsExt operators on encoded state values that the
\* compiled tier evaluates natively: Max, Min, SumSet, MapThenSumSet,
\* SumFunction, IsInjective (both outcomes), checked against definitions
\* by hand.
EXTENDS Naturals, FiniteSets, Functions, FiniteSetsExt
VARIABLES f, g, s
vars == <<f, g, s>>
N == {"a", "b", "c"}
M == {1, 2, 3}
Init == f = [n \in N |-> 0] /\ g = [m \in M |-> 0] /\ s = {}
Next == \E n \in N, m \in M :
          /\ f[n] < 3
          /\ f' = [f EXCEPT ![n] = @ + 1]
          /\ g' = [g EXCEPT ![m] = f[n]]
          /\ s' = IF f[n] \in s THEN s \ {f[n]} ELSE s \cup {f[n] + 1}
Spec == Init /\ [][Next]_vars

RECURSIVE SumR(_)
SumR(S) == IF S = {} THEN 0 ELSE LET x == CHOOSE x \in S : TRUE IN x + SumR(S \ {x})

Inv == /\ s # {} => /\ Max(s) \in s /\ \A y \in s : Max(s) >= y
                    /\ Min(s) \in s /\ \A y \in s : Min(s) <= y
       /\ SumSet(s) = SumR(s)
       /\ MapThenSumSet(LAMBDA x : 2 * x, s) = 2 * SumR(s)
       /\ SumFunction(f) = f["a"] + f["b"] + f["c"]
       /\ SumFunction(g) = g[1] + g[2] + g[3]
       /\ IsInjective(f) = (Cardinality(Range(f)) = Cardinality(N))
       /\ IsInjective(g) = (Cardinality(Range(g)) = Cardinality(M))
====
