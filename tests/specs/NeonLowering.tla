---- MODULE NeonLowering ----
\* Patterns of the Neon ProposerAcceptor specs that the compiled tier
\* lowers natively: DOMAIN of records and of fixed-domain functions in a
\* quantifier over a constant set, `{x} \cup S \cup T` with record
\* elements (more elements than the singleton), `(k :> v) @@ f` with `k`
\* in f's fixed domain, and a growing domain with `@@`.
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS A, NULL, Other
VARIABLES f, h, conf, cs
vars == <<f, h, conf, cs>>

Conf(g, m) == [generation |-> g, members |-> m, newMembers |-> NULL]

Init == /\ f = [a \in A |-> [term |-> 0, next |-> [b \in A |-> NULL]]]
        /\ h = [a \in {} |-> 0]
        /\ conf = Conf(1, A)
        /\ cs = [a \in A |-> Conf(1, A)]

Step(a) == /\ f[a].term < 2
           /\ f' = [f EXCEPT ![a].term = @ + 1,
                             ![a].next = (a :> f[a].term) @@ f[a].next]
           /\ h' = (a :> f[a].term) @@ h
           /\ UNCHANGED <<conf, cs>>

Change(m) == /\ conf.generation < 3
             /\ conf' = [generation |-> conf.generation + 1, members |-> m,
                         newMembers |-> IF m = A THEN NULL ELSE A]
             /\ UNCHANGED <<f, h, cs>>

Adopt(a) == /\ cs[a].generation < conf.generation
            /\ cs' = [cs EXCEPT ![a] = conf]
            /\ UNCHANGED <<f, h, conf>>

Next == \/ \E a \in A : Step(a)
        \/ \E m \in SUBSET A : m /= {} /\ Change(m)
        \/ \E a \in A : Adopt(a)

Spec == Init /\ [][Next]_vars

IsConf(c) == /\ DOMAIN c = {"generation", "members", "newMembers"}
             /\ c.generation \in Nat
             /\ c.members \in SUBSET A
             /\ c.newMembers = NULL \/ c.newMembers \in SUBSET A

Range(g) == {g[x] : x \in DOMAIN g}

TypeOk == /\ \A a \in A : /\ DOMAIN f[a] = {"term", "next"}
                          /\ \A b \in DOMAIN f[a].next : b \in A /\ f[a].next[b] \in Nat \cup {NULL}
                          /\ \A v \in Range(f[a].next) : v = NULL \/ v < 3
          /\ \A a \in DOMAIN h : a \in A /\ h[a] \in Nat
          /\ IsConf(conf)
          /\ \A a \in A : IsConf(cs[a])

AllConfs == {conf} \cup {cs[a] : a \in A} \cup {Conf(1, A)}

ConfigSafety == \A c1, c2 \in AllConfs : c1.generation = c2.generation => c1 = c2

Size == Cardinality(AllConfs) <= 1 + Cardinality(A) + 1
====
