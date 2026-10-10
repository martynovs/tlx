---- MODULE LzSetAlgebra ----
\* Sets of functions, records and tuples over sets TLC cannot enumerate:
\* an empty domain, an empty component or a singleton range decides
\* equality, Cardinality and IsFiniteSet without enumerating the other
\* argument; otherwise sets of the same kind compare by their components
\* (TLC's SetOfFcnsValue/SetOfRcdsValue/SetOfTuplesValue, tlaplus#1407).
\* Also TLCExt!TLCModelValue and TLCExt!TLCFP (fingerprints of equal
\* values are equal).
EXTENDS Integers, Sequences, FiniteSets, TLC, TLCExt

ASSUME [{} -> Nat] = {<<>>} /\ [Nat -> {}] = {}
ASSUME Cardinality([{} -> Nat]) = 1 /\ Cardinality([Nat -> {}]) = 0
ASSUME Cardinality([Nat -> {"d1"}]) = 1 /\ Cardinality([Nat -> SUBSET {}]) = 1
ASSUME Cardinality([[Nat -> {"d1"}] -> {"d2", "d3"}]) = 2
ASSUME Cardinality([(SUBSET (1..40)) -> {"d1"}]) = 1
ASSUME Cardinality([Seq({}) -> {"d1"}]) = 1
ASSUME Cardinality([(UNION {1..2000000, {"a"}}) -> {"d1"}]) = 1
ASSUME Cardinality(SUBSET [(SUBSET (1..40)) -> {"d1"}]) = 2
ASSUME Cardinality([n1 : Nat, n2 : {}]) = 0 /\ Cardinality(Nat \X {}) = 0

ASSUME IsFiniteSet([Nat -> {"d1"}]) /\ IsFiniteSet([STRING -> {"d1"}])
ASSUME IsFiniteSet([Nat -> [Nat -> {"d1"}]]) /\ IsFiniteSet([[Nat -> {"d1"}] -> {"d2", "d3"}])
ASSUME ~IsFiniteSet([{"d1"} -> Nat]) /\ ~IsFiniteSet([Nat -> {"d1", "d2"}])
ASSUME IsFiniteSet(Seq({})) /\ ~IsFiniteSet(Seq({1}))

ASSUME [Nat -> {"d1"}] = [Nat -> {"d1"}] /\ [Nat -> {"d1"}] # [Int -> {"d1"}]
ASSUME [SUBSET Nat -> {"d1"}] = [SUBSET Nat -> {"d1"}]
ASSUME [{"d1"} -> Nat] = [{"d1"} -> Nat] /\ [{"d1"} -> Nat] # [{"d1"} -> Int]
ASSUME [[Nat -> {"d1"}] -> {"d2"}] = [[Nat -> {"d1"}] -> {"d2"}]
ASSUME [[[Nat -> {}] -> {"d1"}] -> {"d2"}] # [[[Nat -> {}] -> {"d1"}] -> {"d3"}]
ASSUME [n1 : Nat, n2 : {"d1"}] = [n1 : Nat, n2 : {"d1"}] /\ [n1 : Nat, n2 : {"d1"}] # [n1 : Int, n2 : {"d1"}]
ASSUME [n1 : Nat, n2 : {}] = [m : {}] /\ [n1 : Nat] # [m : Nat]
ASSUME (Nat \X {"d1"}) = (Nat \X {"d1"}) /\ (Nat \X {"d1"}) # (Int \X {"d1"})
ASSUME (Nat \X {}) = ({} \X STRING)
ASSUME SUBSET Nat = SUBSET Nat /\ SUBSET Nat # SUBSET Int

ASSUME TLCFP([Nat -> {}]) = TLCFP({}) /\ TLCFP([{} -> Nat]) = TLCFP({<<>>})
ASSUME TLCFP([n1 : {"a"}]) = TLCFP([{"n1"} -> {"a"}]) /\ TLCFP(<<1, 2>>) # TLCFP(<<2, 1>>)
ASSUME TLCModelValue("T_MV1") = TLCModelValue("T_MV1") /\ TLCModelValue("T_MV1") # TLCModelValue("T_MV2")
ASSUME ToString(TLCModelValue("mv")) = "mv"

VARIABLE x
Init == x = TLCModelValue("a")
Next == x' = TLCModelValue("b")
Inv == x \in {TLCModelValue("a"), TLCModelValue("b")} /\ Cardinality([Nat -> {x}]) = 1
====
