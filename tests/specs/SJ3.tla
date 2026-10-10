---- MODULE SJ3 ----
\* CSV and IOUtils natives (CommunityModules CSV.java, IOUtils.java):
\* CSVWriteRecord appends the (normalized) field names if asked and the
\* values as TLC prints them; CSVRead splits each line at a Java regex
\* (trailing empty fields dropped) into a record of strings, <<>> for a
\* missing file; CSVWrite formats with String.format; IOEnvExec adds
\* variables to the environment, IOExecTemplate formats each element of the
\* command with the (string) parameters.
EXTENDS CSV, IOUtils, TLC, Sequences, Integers
Sh(cmd) == IOExec(<<"sh", "-c", cmd>>)
ASSUME Sh("rm -rf SJ3-tmp && mkdir SJ3-tmp").exitValue = 0
ASSUME CSVWriteRecord([a |-> "x", b |-> 2, c |-> <<1>>], "#", TRUE, "SJ3-tmp/r.csv")
ASSUME CSVWriteRecord([a |-> "y", b |-> 3, c |-> {}], "#", FALSE, "SJ3-tmp/r.csv")
ASSUME Sh("cat SJ3-tmp/r.csv").stdout = "a#b#c\n\"x\"#2#<<1>>\n\"y\"#3#{}\n"
ASSUME CSVWrite("%1$s|%2$S|%3$5s|%-4s|", <<"q", "w", 1, 2>>, "SJ3-tmp/w.csv")
ASSUME Sh("cat SJ3-tmp/w.csv").stdout = "\"q\"|\"W\"|    1|\"q\" |\n"
ASSUME CSVRecords("SJ3-tmp/w.csv") = 1
ASSUME CSVRecords("SJ3-tmp/none.csv") = 0
ASSUME Sh("printf 'x,y\\n1,2,3\\nq,w,\\n,r\\n' > SJ3-tmp/t.csv").exitValue = 0
ASSUME CSVRead(<<"a", "b">>, ",", "SJ3-tmp/t.csv") =
         <<[a |-> "x", b |-> "y"], [a |-> "1", b |-> "2"], [a |-> "q", b |-> "w"], [a |-> "", b |-> "r"]>>
ASSUME CSVRecords("SJ3-tmp/t.csv") = 4
ASSUME Sh("printf 'a|b\\nc|d|\\n' > SJ3-tmp/p.csv").exitValue = 0
ASSUME CSVRead(<<"a", "b">>, "\\|", "SJ3-tmp/p.csv") = <<[a |-> "a", b |-> "b"], [a |-> "c", b |-> "d"]>>
ASSUME CSVRead(<<"a">>, ",", "SJ3-tmp/none.csv") = <<>>
ASSUME IOEnvExec([FOO |-> "bar", N |-> 3], <<"sh", "-c", "echo $FOO$N">>) = [exitValue |-> 0, stdout |-> "bar3\n", stderr |-> ""]
ASSUME IOExecTemplate(<<"sh", "-c", "echo %1$s-%2$s %s">>, <<"a", "b">>) = [exitValue |-> 0, stdout |-> "a-b a\n", stderr |-> ""]
ASSUME IOEnvExecTemplate([X |-> "1"], <<"sh", "-c", "echo $X %s >&2; exit 4">>, <<"z">>) = [exitValue |-> 4, stdout |-> "", stderr |-> "1 z\n"]
VARIABLE x
Init == x = 0
\* Appended during the search and read back.
Next == /\ x < 3
        /\ CSVWriteRecord([v |-> x], ",", FALSE, "SJ3-tmp/s.csv")
        /\ x' = Len(CSVRead(<<"v">>, ",", "SJ3-tmp/s.csv"))
Post == Sh("rm -rf SJ3-tmp").exitValue = 0
Spec == Init /\ [][Next]_x
====
