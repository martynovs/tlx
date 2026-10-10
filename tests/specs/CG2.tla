---- MODULE CG2 ----
\* In an action, TLC evaluates an application of a definition it
\* overrides by a Java method (SequencesExt!Contains) as a value, with all
\* its arguments, instead of expanding the TLA+ definition (whose body does
\* not use `e` for an empty sequence). The failing argument is an error in
\* state 3, not a deadlock.
EXTENDS Integers, SequencesExt
VARIABLE x, s
Init == x = 0 /\ s = <<>>
Next == x < 3 /\ x' = x + 1 /\ (Contains(s, IF x = 2 THEN CHOOSE n \in {} : TRUE ELSE 1) \/ TRUE) /\ UNCHANGED s
====
