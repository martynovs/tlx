---- MODULE DyrReduce1 ----
\* DyadicRationals: TLC overrides Reduce in Java (tlc2.overrides.DyadicRationals,
\* commons-math gcd): zero numerators, negative values and zero denominators
\* are reduced, unlike with the TLA+ GCD definition (CHOOSE over an empty set).
EXTENDS DyadicRationals, Integers
R(n, d) == [num |-> n, den |-> d]
ASSUME Half(Zero) = R(0, 1)
ASSUME Half(One) = R(1, 2)
ASSUME Half(R(-3, 1)) = R(-3, 2)
ASSUME Half(R(-2, 1)) = R(-1, 1)
ASSUME Half(R(2, -1)) = R(1, -1)
ASSUME Half(R(4, -3)) = R(2, -3)
ASSUME Half(R(6, 0)) = R(1, 0)
ASSUME Half(R(0, 5)) = R(0, 1)
ASSUME Half(R(-6, -3)) = R(-1, -1)
ASSUME Half(R(0, -1)) = R(0, -1)
ASSUME Add(Half(One), Half(One)) = R(1, 1)
ASSUME Add(R(-1, 2), R(1, 2)) = R(0, 1)
ASSUME Add(R(-3, 4), R(1, 4)) = R(-1, 2)
ASSUME Half(R(-2147483647 - 1, 1)) = R(-1073741824, 1)
VARIABLE x
Init == x = Half(Zero)
Next == \/ x' = Half(x)
        \/ x' = Add(x, Half(One))
        \/ x' = Add(x, R(-1, x.den))
        \/ x' = Half(R(-x.num, x.den))
Constraint == x.den <= 16 /\ x.num \in -8..8
====
