---- MODULE US1 ----
\* Order of strings created at run time. TLC orders strings by their
\* UniqueString token, assigned when a string is first created: the spec's
\* tokens at parse time, then strings built by ToString or \o in the order
\* they are created. TLC evaluates the 0-ary constant-level definitions at
\* startup (SpecProcessor.processConstantDefns), so their strings come before
\* those of Init and the actions, even when they are used later or never.
\* The numbers are computed, not written, so that no literal token pre-interns
\* their text.
EXTENDS Naturals, Sequences, SequencesExt, TLC
VARIABLES x, seen, first, seq

Str(n) == ToString(n * 11 + 7)

\* Constant-level: evaluated at startup, in declaration order.
Late == Str(9)           \* "106", used only by the last action
Unused == Str(8) \o "!"  \* "95", "95!": never used
Early == Str(5)          \* "62"

Init ==
    /\ x = 0
    /\ seen = {Str(7), Str(6), "s" \o Str(4)}   \* "84", "73", "51", "s51"
    /\ first = CHOOSE s \in seen : TRUE
    /\ seq = SetToSeq(seen)

Add(S) ==
    /\ seen' = seen \cup S
    /\ first' = CHOOSE s \in seen' : TRUE
    /\ seq' = SetToSeq(seen')

Next ==
    \/ x = 0 /\ x' = 1 /\ Add({Str(3), Str(2)})          \* "40", "29"
    \/ x = 1 /\ x' = 2 /\ Add({Str(1) \o Str(0), Early}) \* "18", "7", "187"
    \/ x = 2 /\ x' = 3 /\ Add({Late, Str(8)})
    \/ x = 3 /\ x' = 4 /\ Add({ToString(<<Str(2), x>>)}) \* "<<\"29\", 3>>"

Inv == x < 4
====
