---- MODULE RS5 ----
\* Files written by CSVWrite in a simulation that the compiled tier starts
\* over after a layout miss (RS5.flags forces misses): every step appends a
\* line; the POSTCONDITION counts them (and removes the file).
EXTENDS Naturals, TLC, CSV, IOUtils
VARIABLES x
ASSUME IOExec(<<"rm", "-f", "RS5.csv">>).exitValue = 0
Init == x = 0
Next == x < 4 /\ CSVWrite("%1$s", <<x>>, "RS5.csv") /\ x' = x + 1
Post == /\ TLCGet("stats").traces = 3
        /\ CSVRecords("RS5.csv") = 12
        /\ IOExec(<<"rm", "-f", "RS5.csv">>).exitValue = 0
====
