---- MODULE PC2 ----
\* A type invariant that sizes the compiled layout is violated only
\* after the pre-run: the violating state is a layout miss first (widen,
\* restart), then the violation is reported with its trace.
EXTENDS Naturals
VARIABLES a, b, msgs

\* (Its strings make the atom universes of the pre-run shapes large: the
\* layout of msgs follows the invariant's bound instead.)
Palette == {"red", "green", "blue", "cyan", "magenta", "yellow", "black", "white", "grey", "pink"}

TypeOK == /\ a \in 0..40
          /\ b \in 0..40
          /\ msgs \in SUBSET [type : {"x"}, c : {"red", "green"}, v : 0..3]

Init == a = 0 /\ b = 0 /\ msgs = {}

Next == \/ a < 40 /\ a' = a + 1 /\ UNCHANGED <<b, msgs>>
        \/ b < 40 /\ b' = b + 1 /\ UNCHANGED <<a, msgs>>
        \/ \E v \in 0..3, c \in {"red", "green"} :
              /\ msgs' = msgs \cup {[type |-> "x", c |-> c, v |-> v]}
              /\ UNCHANGED <<a, b>>
        \/ /\ a = 40 /\ b = 40
           /\ msgs' = msgs \cup {[type |-> "y", c |-> "blue", v |-> 9]}
           /\ UNCHANGED <<a, b>>

Spec == Init /\ [][Next]_<<a, b, msgs>>
====
