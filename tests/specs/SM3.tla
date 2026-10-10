---- MODULE SM3 ----
\* TLC evaluates S \cap T, S \cup T and S \ T without checking that the
\* operands are sets (Tool.java OPCODE_cap/cup/setdiff, Reducible): with an
\* empty enumerated S the other operand is never used, a lazy SetCupValue
\* fails only when it is used.
EXTENDS Naturals, Sequences, TLC
VARIABLES x, n
Init == /\ x = <<"c">>
        /\ n = 0
Next == /\ n < 2
        /\ n' = n + 1
        /\ x' = IF n = 0 THEN ({} \cup x) \o <<"d">> ELSE x
Inv == /\ FALSE \notin ({} \cap x)
       /\ ({} \cap x) = {}
       /\ ({} \ x) = {}
       /\ ({} \cup x) = x
       /\ 1 \in ({1} \cup x)
       /\ 1 \in (x \cup {1})
       /\ 1 \notin ({} \cap 3) \cup {2}
       /\ LET S == FALSE \cup x IN Len(x) > 0
       /\ \A y \in {} \cap x : FALSE
       /\ ToString(({} \cap 3) \cup {1}) = "{1}"
       /\ ToString([y \in Nat |-> y]) # ""
====
