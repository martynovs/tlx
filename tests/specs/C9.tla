---- MODULE C9 ----
\* RECURSIVE operators as compiled functions (linear, mutual, on encoded
\* sets and sequences), and path counting of non-assigning branching
\* conjuncts with a recursive lazily applied function (VoteProof's SafeAt).
EXTENDS Integers, Sequences, FiniteSets
VARIABLES s, q, n, votes

Vals == 0..2

RECURSIVE SumSet(_)
SumSet(S) == IF S = {} THEN 0 ELSE LET x == CHOOSE x \in S : TRUE IN x + SumSet(S \ {x})

RECURSIVE Rev(_)
Rev(t) == IF t = <<>> THEN <<>> ELSE Rev(Tail(t)) \o <<Head(t)>>

RECURSIVE Even(_), Odd(_)
Even(k) == IF k = 0 THEN TRUE ELSE Odd(k - 1)
Odd(k) == IF k = 0 THEN FALSE ELSE Even(k - 1)

RECURSIVE Chain(_, _)
Chain(S, k) == IF k \in S /\ k > 0 THEN Chain(S, k - 1) + 1 ELSE 0

Ok(b, v) ==
  LET SA[bb \in 0..3] ==
        \/ bb = 0
        \/ \E c \in -1..(bb - 1) :
             /\ (c # -1) => SA[c]
             /\ c \in s \/ v \in s
  IN SA[b]

Init == s = {} /\ q = <<>> /\ n = 0 /\ votes = {}

Add == \E x \in Vals : x \notin s /\ s' = s \cup {x} /\ UNCHANGED <<q, n, votes>>
Del == \E x \in s : s' = s \ {x} /\ UNCHANGED <<q, n, votes>>
Push == Len(q) < 3 /\ \E x \in Vals : q' = Append(q, x) /\ UNCHANGED <<s, n, votes>>
Flip == Len(q) > 1 /\ q' = Rev(q) /\ UNCHANGED <<s, n, votes>>
Count == n' = (SumSet(s) + Chain(s, 2) + (IF Even(Len(q)) THEN 1 ELSE 0)) % 5 /\ UNCHANGED <<s, q, votes>>
Vote == \E b \in 0..2, v \in Vals :
          /\ Ok(b, v)
          /\ Cardinality(votes) < 2
          /\ votes' = votes \cup {<<b, v>>}
          /\ UNCHANGED <<s, q, n>>
Forget == \E x \in votes : votes' = votes \ {x} /\ UNCHANGED <<s, q, n>>

Next == Add \/ Del \/ Push \/ Flip \/ Count \/ Vote \/ Forget

Inv == /\ SumSet(s) <= 6
       /\ Len(Rev(q)) = Len(q)
       /\ Even(Len(q)) # Odd(Len(q))
       /\ \A x \in votes : x[1] \in 0..3
====
