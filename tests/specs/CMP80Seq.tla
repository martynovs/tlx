------------------------------ MODULE CMP80Seq ------------------------------
(* SetToSeq of a set mapped from a state variable (as in            *)
(* KeyValueStore/KVsnap): the CHOOSE over [1..n -> S] is the least          *)
(* elements of S in TLC order. Stops at a violation, so the counts depend   *)
(* on the order of the successors.                                          *)
EXTENDS Naturals, Sequences, FiniteSets
CONSTANTS Keys, Tx, NoVal
VARIABLES store, rk, ops, pc
vars == <<store, rk, ops, pc>>

IsInjective(f) == \A a, b \in DOMAIN f : f[a] = f[b] => a = b
SetToSeq(S) == CHOOSE f \in [1..Cardinality(S) -> S] : IsInjective(f)
Op(o, k, v) == [op |-> o, key |-> k, value |-> v]

Init == /\ store = [k \in Keys |-> NoVal]
        /\ rk = [t \in Tx |-> {}]
        /\ ops = [t \in Tx |-> <<>>]
        /\ pc = [t \in Tx |-> "start"]

Start(t) == /\ pc[t] = "start"
            /\ \E S \in SUBSET Keys \ {{}} : rk' = [rk EXCEPT ![t] = S]
            /\ pc' = [pc EXCEPT ![t] = "read"]
            /\ UNCHANGED <<store, ops>>

Read(t) == /\ pc[t] = "read"
           /\ ops' = [ops EXCEPT ![t] = ops[t] \o SetToSeq({Op("read", k, store[k]) : k \in rk[t]})]
           /\ pc' = [pc EXCEPT ![t] = "write"]
           /\ UNCHANGED <<store, rk>>

Write(t) == /\ pc[t] = "write"
            /\ store' = [k \in Keys |-> IF k \in rk[t] THEN t ELSE store[k]]
            /\ ops' = [ops EXCEPT ![t] = ops[t] \o SetToSeq({Op("write", k, t) : k \in rk[t]})]
            /\ pc' = [pc EXCEPT ![t] = "done"]
            /\ UNCHANGED rk

Next == \E t \in Tx : Start(t) \/ Read(t) \/ Write(t)

Spec == Init /\ [][Next]_vars

Short == \A t \in Tx : Len(ops[t]) < 5
=============================================================================
