------------------------------ MODULE CMP81Fold ------------------------------
(* FoldFunction / FoldSeq unrolled over encoded functions, records, *)
(* tuples and sequences. Mix is order-sensitive, so the reachable states    *)
(* (guards on folds) depend on TLC's fold order: acc = op(f[i], acc) for i  *)
(* in TLC's order of DOMAIN f. Weights are DyadicRationals summed as in     *)
(* Huang (the accumulator leaves the integer ranges of the elements).       *)
EXTENDS Naturals, Sequences, Functions, SequencesExt, DyadicRationals
CONSTANTS P
VARIABLES w, s, r, t

Mix(x, acc) == (acc * 3 + x) % 101

Init == /\ w = [p \in P |-> Zero]
        /\ s = <<>>
        /\ r = [zz |-> 0, aa |-> 1, mm |-> 2]
        /\ t = <<0, 1, 2>>

Throw(p) == /\ w[p].den < 8
            /\ w' = [w EXCEPT ![p] = IF @ = Zero THEN Half(One) ELSE Add(@, Half(@))]
            /\ UNCHANGED <<s, r, t>>

Push(k) == /\ Len(s) < 4
           /\ FoldFunction(Mix, 0, s) % 5 # k
           /\ s' = Append(s, k)
           /\ UNCHANGED <<w, r, t>>

Pop == /\ s # <<>>
       /\ FoldSeq(Mix, 1, s) % 3 # 0
       /\ s' = Tail(s)
       /\ UNCHANGED <<w, r, t>>

Rot == /\ FoldFunction(Mix, 0, r) # FoldFunction(Mix, 0, t)
       /\ r' = [zz |-> r.aa, aa |-> r.mm, mm |-> (r.zz + Len(s)) % 4]
       /\ t' = <<t[2], t[3], (t[1] + FoldFunction(LAMBDA x, a : x.den + a, 0, w)) % 4>>
       /\ UNCHANGED <<w, s>>

Next == \/ \E p \in P : Throw(p)
        \/ \E k \in 1..3 : Push(k)
        \/ Pop
        \/ Rot

Spec == Init /\ [][Next]_<<w, s, r, t>>

\* The sum of the weights (as in Huang's TypeOK) and a fold over a function
\* of model values with an order-sensitive operator.
Total == FoldFunction(Add, Zero, w)
Heavy == Total.num <= 3 * Total.den
Order == FoldFunction(Mix, 0, [p \in P |-> w[p].den]) # 77 \/ Len(s) < 4
=============================================================================
