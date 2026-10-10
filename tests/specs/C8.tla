---- MODULE C8 ----
\* Tuple binders <<x, y>> \in S in quantifiers, CHOOSE, set constructors,
\* function constructors and function definitions; recursion over
\* constant sets; IF/CASE with constant conditions.
EXTENDS Integers, Sequences, FiniteSets
VARIABLES grid, edges, q

N == 3
Pos == (1..N) \X (1..N)

RECURSIVE Sum(_, _)
Sum(f, S) == IF S = {} THEN 0
             ELSE LET x == CHOOSE x \in S : TRUE IN f[x] + Sum(f, S \ {x})

cell[<<x, y>> \in (0..N+1) \X (0..N+1)] ==
    CASE x = 0 \/ y = 0 \/ x > N \/ y > N -> 0
      [] grid[<<x, y>>] -> 1
      [] OTHER -> 0

Nbrs(p) == {<<p[1] + dx, p[2] + dy>> : <<dx, dy>> \in {<<-1, 0>>, <<1, 0>>, <<0, -1>>, <<0, 1>>}}

Init == /\ grid \in {[p \in Pos |-> p[1] = p[2]], [p \in Pos |-> FALSE]}
        /\ edges = {}
        /\ q = <<>>

Flip == \E <<x, y>> \in Pos :
          /\ Sum(cell, Nbrs(<<x, y>>)) \in 0..2
          /\ grid' = [grid EXCEPT ![<<x, y>>] = ~@]
          /\ UNCHANGED <<edges, q>>

Link == \E <<a, b>> \in {<<u, v>> \in Pos \X Pos : u[1] = v[1] /\ u[2] + 1 = v[2]} :
          /\ grid[a] /\ grid[b]
          /\ Cardinality(edges) < 2
          /\ edges' = edges \cup {<<a, b>>}
          /\ UNCHANGED <<grid, q>>

Push == /\ Len(q) < 2
        /\ \E e \in edges : q' = Append(q, e)
        /\ UNCHANGED <<grid, edges>>

Pop == \E <<a, b>> \in {q[i] : i \in DOMAIN q} :
          /\ q' = SelectSeq(q, LAMBDA e : e /= <<a, b>>)
          /\ edges' = edges \ {<<a, b>>}
          /\ UNCHANGED grid

Next == Flip \/ Link \/ Push \/ Pop

Ends == {b : <<a, b>> \in edges}
Starts == {a \in Pos : \E <<u, v>> \in edges : u = a}
Deg == [<<x, y>> \in Pos |-> Cardinality({e \in edges : e[1] = <<x, y>>})]

Inv == /\ \A <<a, b>> \in edges : a[1] = b[1] /\ a[2] + 1 = b[2]
       /\ \A <<x, y>> \in Pos : Deg[<<x, y>>] <= 2
       /\ Cardinality(Ends) <= Cardinality(edges)
       /\ Starts \subseteq Pos
       /\ edges = {} \/ (CHOOSE <<a, b>> \in edges : TRUE) \in edges
       /\ \A e \in edges : e \in {<<a, b>> \in Pos \X Pos : b[2] = a[2] + 1}
       /\ Sum(cell, Pos) = Cardinality({p \in Pos : grid[p]})
       /\ \A i \in DOMAIN q : q[i] \in edges \/ TRUE
====
