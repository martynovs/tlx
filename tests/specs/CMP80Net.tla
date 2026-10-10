------------------------------ MODULE CMP80Net ------------------------------
(* A set of messages that are records with a set-valued field       *)
(* (bitset-encoded, ranked through the field's bitset), built, tested and   *)
(* iterated in actions, invariants and the fairness conditions of the       *)
(* liveness check (as in allocator/AllocatorImplementation).                *)
EXTENDS Naturals, FiniteSets
CONSTANTS Clients, Resources
VARIABLES network, holding, requests
vars == <<network, holding, requests>>

Messages == [type : {"request", "allocate", "return"}, clt : Clients, rsrc : SUBSET Resources]

Init == /\ network = {}
        /\ holding = [c \in Clients |-> {}]
        /\ requests = [c \in Clients |-> {}]

Request(c, S) ==
  /\ requests[c] = {} /\ holding[c] = {} /\ S # {}
  /\ requests' = [requests EXCEPT ![c] = S]
  /\ network' = network \cup {[type |-> "request", clt |-> c, rsrc |-> S]}
  /\ UNCHANGED holding

Busy == UNION {holding[c] : c \in Clients} \cup UNION {m.rsrc : m \in {x \in network : x.type = "allocate"}}

Grant(m) ==
  /\ m.type = "request" /\ m.rsrc \cap Busy = {}
  /\ network' = (network \ {m}) \cup {[type |-> "allocate", clt |-> m.clt, rsrc |-> m.rsrc]}
  /\ UNCHANGED <<holding, requests>>

Receive(m) ==
  /\ m.type = "allocate"
  /\ holding' = [holding EXCEPT ![m.clt] = @ \cup m.rsrc]
  /\ network' = network \ {m}
  /\ UNCHANGED requests

Return(c) ==
  /\ holding[c] # {}
  /\ network' = network \cup {[type |-> "return", clt |-> c, rsrc |-> holding[c]]}
  /\ holding' = [holding EXCEPT ![c] = {}]
  /\ requests' = [requests EXCEPT ![c] = {}]

Ack(m) == m.type = "return" /\ network' = network \ {m} /\ UNCHANGED <<holding, requests>>

Next ==
  \/ \E c \in Clients, S \in SUBSET Resources : Request(c, S)
  \/ \E m \in network : Grant(m) \/ Receive(m) \/ Ack(m)
  \/ \E c \in Clients : Return(c)

Spec ==
  /\ Init /\ [][Next]_vars
  /\ \A c \in Clients : WF_vars(Return(c))
  /\ \A c \in Clients : WF_vars(\E S \in SUBSET Resources : Request(c, S))
  /\ WF_vars(\E m \in network : Grant(m) \/ Receive(m) \/ Ack(m))

TypeOK == network \in SUBSET Messages /\ holding \in [Clients -> SUBSET Resources]

Mutex == \A c1, c2 \in Clients : c1 # c2 => holding[c1] \cap holding[c2] = {}

Pending == \A c \in Clients :
             [type |-> "request", clt |-> c, rsrc |-> requests[c]] \in network => holding[c] = {}

Served == \A c \in Clients : requests[c] # {} ~> holding[c] # {}
=============================================================================
