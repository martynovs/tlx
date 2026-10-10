---- MODULE LS6 ----
\* LS4 as first written, with constant concatenations. Run-time
\* strings are ordered by creation: the search creates "qx", "qxx",
\* then "qy", so the CHOOSE below picks "qxx" (TLC: no error). With a small
\* shape pre-run (TLX_PRERUN=1) the compiled tier folded the concatenations
\* at compile time in source order, "qy" before "qxx", and Inv failed.
EXTENDS Naturals, Sequences, TLC
CONSTANTS a, b
VARIABLES p, s, n
Sym == Permutations({a, b})
Init == p \in {a, b} /\ s = "" /\ n = 0
Next == /\ n < 5
        /\ UNCHANGED p
        /\ s' = CASE n = 0 -> IF p = a THEN "q" \o "x" ELSE "q" \o "y"
                  [] n = 1 -> s \o "x"
                  [] n = 2 -> "q" \o "y"
                  [] OTHER -> s
        /\ n' = IF n = 3 /\ (CHOOSE t \in {"q" \o "y", ("q" \o "x") \o "x"} : TRUE) = "q" \o "y" THEN 7 ELSE n + 1
Inv == n # 7
====
