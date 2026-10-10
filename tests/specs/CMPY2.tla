---- MODULE CMPY2 ----
\* 0-ary definitions applying higher-order operators: state-level (and
\* cacheable per state) when the operators passed are, action-level when
\* a LAMBDA or a definition passed reads the next state.
EXTENDS Naturals, Sequences
VARIABLES q, n
MapSeq(s, Op(_)) == [i \in DOMAIN s |-> Op(s[i])]
Twice(Op(_), x) == Op(Op(x))
Inc(x) == x + 1
NextInc(x) == x + n'
Doubled == MapSeq(q, LAMBDA x : 2 * x)
Bumped == MapSeq(q, Inc)
Shifted == MapSeq(q, LAMBDA x : x + n')
Pushed == MapSeq(q, NextInc)
Init == q = <<1, 2>> /\ n = 0
Next == /\ n < 3
        /\ n' = n + 1
        /\ q' = IF Len(Shifted) = Len(Pushed) /\ Shifted = Pushed
                THEN Append(Doubled, Twice(Inc, n))
                ELSE q
Inv == /\ \A i \in DOMAIN q : Doubled[i] = 2 * q[i] /\ Bumped[i] = q[i] + 1
       /\ Len(Doubled) = Len(q)
====
