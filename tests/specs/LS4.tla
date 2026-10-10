---- MODULE LS4 ----
\* Run-time strings are ordered by creation, so the
\* compiled tier's faithful pre-run must create them in the search's order.
\* Under SYMMETRY the search expands one of the two symmetric initial states;
\* a pre-run without the reduction expanded both and created "qy" before
\* "qxx", so the CHOOSE below picked "qy" and Inv failed (TLC: no error).
EXTENDS Naturals, Sequences, TLC
CONSTANTS a, b
\* e is always "": a concatenation with it is not a constant expression, so
\* the compiled tier does not fold it (creating its string) at compile time.
VARIABLES p, s, n, e
Sym == Permutations({a, b})
Init == p \in {a, b} /\ s = "" /\ n = 0 /\ e = ""
Next == /\ n < 5
        /\ UNCHANGED <<p, e>>
        /\ s' = CASE n = 0 -> IF p = a THEN (e \o "q") \o "x" ELSE (e \o "q") \o "y"
                  [] n = 1 -> s \o "x"
                  [] n = 2 -> (e \o "q") \o "y"
                  [] OTHER -> s
        /\ n' = IF n = 3 /\ (CHOOSE t \in {(e \o "q") \o "y", ((e \o "q") \o "x") \o "x"} : TRUE) = (e \o "q") \o "y" THEN 7 ELSE n + 1
Inv == n # 7
====
