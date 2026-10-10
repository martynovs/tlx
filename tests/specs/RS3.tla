---- MODULE RS3 ----
\* Files written by CSVWrite across a resumed compiled search
\* (RS3.flags forces layout misses): every expansion of a state with
\* x < 6 appends a line; the POSTCONDITION counts them (and removes the
\* file).
EXTENDS Naturals, TLC, CSV, IOUtils
VARIABLES x, y
ASSUME IOExec(<<"rm", "-f", "RS3.csv">>).exitValue = 0
Init == x = 0 /\ y = 0
Next == \/ x < 6 /\ CSVWrite("%1$s,%2$s", <<x, y>>, "RS3.csv") /\ x' = x + 1 /\ y' = y
        \/ y < 4 /\ y' = y + 1 /\ x' = x
Post == /\ TLCGet("stats").distinct = 35
        /\ CSVRecords("RS3.csv") = 30
        /\ IOExec(<<"rm", "-f", "RS3.csv">>).exitValue = 0
====
