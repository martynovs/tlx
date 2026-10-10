---- MODULE JA2 ----
\* Loops over a sorted set of messages of several record types (a universe
\* too large for a bitset; the JIT specializes the body per arm with
\* a branch on the tag).
EXTENDS Naturals, FiniteSets
Msg == [type : {"a"}, x : 0..99, y : 0..59]
       \cup [type : {"b"}, x : 0..99]
       \cup [type : {"c"}, z : 0..3]
VARIABLES msgs, n
Init == msgs = {} /\ n = 0
SendA == /\ n < 4
         /\ msgs' = msgs \cup {[type |-> "a", x |-> n * 33, y |-> n * 19]}
         /\ n' = n + 1
SendB == /\ \E m \in msgs : /\ m.type = "a"
                            /\ m.y < 40
                            /\ msgs' = msgs \cup {[type |-> "b", x |-> m.x + 1]}
         /\ UNCHANGED n
SendC == /\ \E m \in msgs : /\ m.type = "b"
                            /\ ~\E k \in msgs : k.type = "c" /\ k.z = m.x % 4
                            /\ msgs' = msgs \cup {[type |-> "c", z |-> m.x % 4]}
         /\ UNCHANGED n
Next == SendA \/ SendB \/ SendC
TypeOK == /\ msgs \subseteq Msg
          /\ \A m \in msgs : \/ m.type = "a" /\ m.x \in 0..99
                             \/ m.type = "b"
                             \/ m.type = "c" /\ m.z \in 0..3
Inv == /\ Cardinality({m \in msgs : m.type = "c"}) <= 4
       /\ \A m \in msgs : m.type = "b" => \E a \in msgs : a.type = "a" /\ a.x + 1 = m.x
====
