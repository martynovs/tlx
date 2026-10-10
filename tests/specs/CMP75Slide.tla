---- MODULE CMP75Slide ----
\* A small sliding puzzle in the style of SlidingPuzzles: UNION of
\* a set of sets in a state variable, a CHOOSE with a unique witness
\* (ChooseOne), set maps over pieces and board' \in a set of sets of sets
\* (successors in TLC's order of sets: the violation stops the search early,
\* so the counts depend on it).
EXTENDS Integers

VARIABLE board

W == 4 H == 3
Pos == (0 .. W - 1) \X (0 .. H - 1)

Start == {{<<0, 0>>, <<0, 1>>}, {<<1, 0>>, <<2, 0>>}, {<<3, 0>>},
          {<<1, 1>>}, {<<2, 1>>, <<3, 1>>}, {<<0, 2>>}}

Goal == {<<3, 1>>, <<3, 2>>} \notin board

ChooseOne(S, P(_)) == CHOOSE x \in S : P(x) /\ \A y \in S : P(y) => y = x

TypeOK == \A pc \in board : pc # {} /\ pc \subseteq Pos

dir(p, es) == LET dir == {<<1, 0>>, <<0, 1>>, <<-1, 0>>, <<0, -1>>}
              IN {d \in dir : /\ <<p[1] + d[1], p[2] + d[2]>> \in Pos
                              /\ <<p[1] + d[1], p[2] + d[2]>> \notin es}

move(p, d) == LET s == <<p[1] + d[1], p[2] + d[2]>>
                  pc == ChooseOne(board, LAMBDA pc : s \in pc)
              IN <<pc, {<<q[1] - d[1], q[2] - d[2]>> : q \in pc}>>

update(e, es) == LET dirs  == dir(e, es)
                     moved == {move(e, d) : d \in dirs}
                     free  == {<<pc, m>> \in moved :
                                 /\ m \cap (UNION (board \ {pc})) = {}
                                 /\ \A p \in m : p \in Pos}
                 IN {(board \ {pc}) \cup {m} : <<pc, m>> \in free}

Init == board = Start

Next == LET empty == Pos \ UNION board
        IN  \E e \in empty : board' \in update(e, empty)
====
