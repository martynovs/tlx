---- MODULE AP1 ----
\* The Apalache module: TLC evaluates its TLA+ definitions (no overrides).
EXTENDS Apalache, Integers, Sequences, FiniteSets
VARIABLES x, y
Add(a, b) == a + b
Cat(s, e) == s \o <<e>>
Sq(i) == i * i
Step(v, i) == v + i
ASSUME Gen(3) = {}
ASSUME Guess({3, 1, 2}) = 1
ASSUME SetAsFun({<<1, "a">>, <<2, "b">>}) = <<"a", "b">>
ASSUME MkSeq(4, Sq) = <<1, 4, 9, 16>>
ASSUME FunAsSeq([i \in 1..5 |-> i + 10], 3, 5) = <<11, 12, 13>>
ASSUME Skolem(\E i \in 1..3 : i > 2) /\ Expand(SUBSET {1, 2}) = SUBSET {1, 2}
ASSUME ConstCardinality(Cardinality({1, 2}) >= 2)
ASSUME ApaFoldSet(Add, 0, 1..10) = 55 /\ ApaFoldSet(Cat, <<>>, {3, 1, 2}) = <<1, 2, 3>>
ASSUME ApaFoldSeqLeft(Cat, <<0>>, <<3, 1, 2>>) = <<0, 3, 1, 2>>
ASSUME Repeat(Step, 4, 100) = 110
\* `:=` is `=`, so it assigns in Init and Next.
Init == /\ x := 0
        /\ y \in {1, 2}
Next == /\ x < 3
        /\ x' := x + 1
        /\ y' := Guess({y, 7} \ {x})
====
