---- MODULE SN16 ----
\* Fuzzer finding: an EXCEPT value that does not fit the slot's
\* layout (here a field holding <<1>> set to sequences of other lengths)
\* is a runtime layout miss in the compiled tier, which bails out to the
\* interpreter; Len of the field (a fixed-length tuple in the layout) was
\* folded to the old length without evaluating the EXCEPT. w is <<>> in
\* every state (as in the finding), u ranges over several lengths.
EXTENDS Naturals, Sequences
VARIABLES v, w, u, r
Init == /\ v = [a |-> <<1>>, c |-> 2]
        /\ w = <<>>
        /\ u \in {<<>>, <<3>>, <<4, 5>>}
        /\ r = <<>>
Next == /\ r = <<>>
        /\ r' = << Len(([v EXCEPT !.a = w]).a),
                   Len(([v EXCEPT !.a = u]).a),
                   Len([v EXCEPT !.a = <<1, 2, 3>>].a),
                   Len([v EXCEPT !.a = @ \o @].a),
                   Len([[q |-> v] EXCEPT !.q.a = u].q.a),
                   LET e == [v EXCEPT !.a = u] IN Len(e.a) + Len(e.a),
                   LET x == ([v EXCEPT !.a = u]).a IN Len(x) >>
        /\ UNCHANGED <<v, w, u>>
Lens == r = <<>> \/ r = <<0, Len(u), 3, 2, Len(u), 2 * Len(u), Len(u)>>
EmptyField == /\ Len(([v EXCEPT !.a = w]).a) = 0
              /\ DOMAIN ([v EXCEPT !.a = w]).a = {}
              /\ ([v EXCEPT !.a = w]).a \o <<7>> = <<7>>
              /\ Append(([v EXCEPT !.a = w]).a, 8) = <<8>>
              /\ Len([[v EXCEPT !.a = w] EXCEPT !.c = 3].a) = 0
              /\ LET x == ([v EXCEPT !.a = w]).a IN Len(x) = 0
Field == /\ Len(([v EXCEPT !.a = u]).a) = Len(u)
         /\ DOMAIN ([v EXCEPT !.a = u]).a = 1..Len(u)
         /\ ([v EXCEPT !.a = u]).a \o <<7>> = u \o <<7>>
         /\ Append(([v EXCEPT !.a = u]).a, 8) = Append(u, 8)
         /\ u # <<>> => Head(([v EXCEPT !.a = u]).a) = Head(u)
         /\ u # <<>> => Tail(([v EXCEPT !.a = u]).a) = Tail(u)
         /\ Len([v EXCEPT !.a = Tail(@)].a) = 0
         /\ Len(([[q |-> v] EXCEPT !.q.a = u]).q.a) = Len(u)
         /\ Len(([[q |-> [p |-> v]] EXCEPT !.q.p.a = u]).q.p.a) = Len(u)
         /\ LET e == [v EXCEPT !.a = u]
                x == e.a
            IN Len(x) = Len(u) /\ DOMAIN x = DOMAIN u
====
