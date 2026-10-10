---- MODULE CG4 ----
\* Set operations on encoded sets of different element shapes are
\* compiled by converting one operand into the other's bitset universe: a
\* set of sequences (of 0..3) into a bitset of triples (of 1..3), as in
\* N-Queens/QueensPluscal's `sols \cup exts`. A sequence that does not fit
\* the triples (the empty sequence in `todo`) fails the compiled evaluation;
\* the interpreter then evaluates it.
EXTENDS Integers, Sequences
VARIABLES todo, sols
vars == <<todo, sols>>
Init == todo = {<<>>} /\ sols = {}
Next == \E q \in todo :
          LET exts == {Append(q, c) : c \in 1..3}
          IN IF Len(q) + 1 = 3
                THEN todo' = todo \ {q} /\ sols' = sols \cup exts
                ELSE todo' = (todo \ {q}) \cup exts /\ sols' = sols
Grows == [][\E q \in todo : sols' = sols \/ sols' = sols \cup {Append(q, c) : c \in 1..3}]_vars
Mixed == [][sols' \cup todo # {} \/ todo = {}]_vars
Inv == sols \subseteq {s \in (1..3) \X (1..3) \X (1..3) : TRUE}
====
