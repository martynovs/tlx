---- MODULE LzBigRandomSubset ----
\* RandomSubset of an interval, a product, a set of functions or of records
\* picks elements by index (TLC: IntervalValue / SetOfFcnsOrRcdsValue)
\* instead of enumerating the set, also beyond 2^31 - 1 elements; an empty
\* one decides before its components are enumerated. Operators applied to
\* huge intervals in ASSUME are not slowed down by memoization.
EXTENDS Integers, FiniteSets, Randomization

MaxInt == 2147483647
Big == (1..4000) \X (1..4000) \X (1..4000)
Fcns == [1..9 -> 1..10]
Rcds == [a : 1..50000, b : 1..50000]
SingletonOf(T, S) == T \subseteq S /\ T # {} /\ \A x, y \in T : x = y

ASSUME Cardinality(RandomSubset(100, Big)) = 100 /\ RandomSubset(100, Big) \subseteq Big
ASSUME Cardinality(RandomSubset(1000, Fcns)) = 1000 /\ RandomSubset(1000, Fcns) \subseteq Fcns
ASSUME Cardinality(RandomSubset(10, Rcds)) = 10 /\ RandomSubset(10, Rcds) \subseteq Rcds
ASSUME Cardinality(RandomSubset(1001, 1..100000000)) = 1001
ASSUME RandomSubset(1, [((1..50000) \X (1..50000)) -> {}]) = {}
ASSUME SingletonOf(RandomSubset(1, 1..(MaxInt - 1)), 1..(MaxInt - 1))

VARIABLE x
Init == x \in RandomSubset(3, Fcns)
Next == UNCHANGED x
Inv == x \in Fcns
====
