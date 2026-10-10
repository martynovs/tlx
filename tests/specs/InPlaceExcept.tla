---- MODULE InPlaceExcept ----
\* Chained and nested EXCEPTs, set insertions and unions whose results the
\* compiled tier computes in place: later updates read the value
\* before earlier ones, LET-bound intermediate values are read after the
\* updates that follow them, sequence updates outside the length.
EXTENDS Naturals, Sequences, FiniteSets
P == {"p1", "p2"}
VARIABLES st, sq, msgs, bits

Init ==
  /\ st = [p \in P |-> [a |-> 0, b |-> 1, c |-> "x"]]
  /\ sq = <<0, 1>>
  /\ msgs = {}
  /\ bits = {}

\* A swap: the second update reads the old field.
Swap(p) ==
  /\ st' = [st EXCEPT ![p].a = st[p].b, ![p].b = st[p].a]
  /\ UNCHANGED <<sq, msgs, bits>>

\* The same swap on an intermediate value computed by an update.
SwapLet(p, q) ==
  /\ LET g == [st EXCEPT ![q].c = IF @ = "x" THEN "y" ELSE "x"]
     IN st' = [g EXCEPT ![p].a = g[p].b, ![p].b = g[p].a, ![q].a = (g[q].a + g[p].a) % 2]
  /\ UNCHANGED <<sq, msgs, bits>>

\* The same inside the assigned value.
SwapIn(p, q) ==
  /\ st' = LET g == [st EXCEPT ![q].c = IF @ = "x" THEN "y" ELSE "x"]
           IN [g EXCEPT ![p].a = g[p].b, ![p].b = g[p].a, ![q].a = (g[q].a + g[p].a) % 2]
  /\ UNCHANGED <<sq, msgs, bits>>

\* Nested paths with @ and a later update of the same slot.
Bump(p) ==
  /\ st' = [st EXCEPT ![p].a = (@ + 1) % 2, ![p] = [@ EXCEPT !.b = (@ + st[p].a) % 2]]
  /\ UNCHANGED <<sq, msgs, bits>>

\* Sequence updates (an index beyond the length leaves it unchanged).
Shift(i) ==
  /\ sq' = IF Len(sq) < 3 /\ i = 3 THEN Append(sq, 1)
           ELSE [[sq EXCEPT ![i] = (@ + 1) % 2] EXCEPT ![1] = sq[Len(sq)]]
  /\ UNCHANGED <<st, msgs, bits>>

\* Two insertions, the second element read from the next state.
Send(p) ==
  /\ st' = [st EXCEPT ![p].c = "y"]
  /\ msgs' = msgs \cup {[from |-> p, v |-> st[p].a], [from |-> p, v |-> st'[p].b]}
  /\ Cardinality(msgs') <= 2
  /\ UNCHANGED <<sq, bits>>

\* A union bound by LET, checked, then assigned.
Mark(p) ==
  /\ LET u == bits \cup {st[p].a, st[p].b}
     IN /\ Cardinality(u) <= 2
        /\ bits' = u
  /\ UNCHANGED <<st, sq, msgs>>

Next ==
  \/ \E p \in P : Swap(p) \/ Bump(p) \/ Send(p) \/ Mark(p)
  \/ \E p, q \in P : p # q /\ (SwapLet(p, q) \/ SwapIn(p, q))
  \/ \E i \in 1..3 : Shift(i)

Inv == \A p \in P : st[p].a \in 0..1 /\ st[p].b \in 0..1
====
