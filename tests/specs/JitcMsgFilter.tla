---- MODULE JitcMsgFilter ----
\* Loops over a bitset of union elements (messages of several kinds with
\* different fields) whose predicate starts with tests of constant fields:
\* filters, quantifiers, CHOOSE and set maps, in a guard, an update and the
\* invariants. The native tiers visit only the ranks that can pass the tests.
EXTENDS Naturals, FiniteSets
VARIABLES msgs, n

Msg == [type : {"a"}, bal : 0..2, val : 0..2]
         \cup [type : {"b"}, bal : 0..2]
         \cup [type : {"c"}, src : 0..1, bal : 0..2, val : 0..1]
         \cup [type : {"d"}]

TypeOK == /\ msgs \subseteq Msg
          /\ n \in 0..6

Init == /\ msgs = {}
        /\ n = 0

SendA(b, v) == /\ n < 5
               /\ msgs' = msgs \cup {[type |-> "a", bal |-> b, val |-> v]}
               /\ n' = n + 1

SendB(b) == /\ n < 5
            /\ msgs' = msgs \cup {[type |-> "b", bal |-> b]}
            /\ n' = n + 1

\* An existential over the elements of one kind and one ballot.
Reply == /\ n < 5
         /\ \E m \in msgs : m.type = "a" /\ m.bal = 1 /\ m.val >= 1
         /\ msgs' = msgs \cup {[type |-> "c", src |-> 1, bal |-> 2, val |-> 1]}
         /\ n' = n + 1

\* A filter and a map of the elements of one kind.
Ack == /\ n < 5
       /\ {m \in msgs : m.type = "b" /\ m.bal = 2} # {}
       /\ msgs' = msgs \cup {[type |-> "c", src |-> 0, bal |-> x, val |-> 0] :
                              x \in {m.bal : m \in {q \in msgs : q.type = "b" /\ q.bal >= 1}}}
                  \cup {[type |-> "d"]}
       /\ n' = n + 1

\* CHOOSE over the elements of one kind (one of them).
Pick == /\ n < 5
        /\ \E m \in msgs : m.type = "c" /\ m.src = 1
        /\ LET m == CHOOSE q \in msgs : q.type = "c" /\ q.src = 1
           IN msgs' = (msgs \ {m}) \cup {[type |-> "b", bal |-> m.bal]}
        /\ n' = n + 1

Next == \/ \E b \in 0..2, v \in 0..2 : SendA(b, v)
        \/ \E b \in 0..2 : SendB(b)
        \/ Reply \/ Ack \/ Pick

Spec == Init /\ [][Next]_<<msgs, n>>

\* Invariants over the same shapes of loops (quantifiers, filter, cardinality).
AllBalOk == \A m \in msgs : m.type = "b" => m.bal <= 2
NoDupC == Cardinality({m \in msgs : m.type = "c" /\ m.src = 1 /\ m.bal = 2}) <= 1
SomeA == (\E m \in msgs : m.type = "a" /\ m.val = 2) => n >= 1
====
