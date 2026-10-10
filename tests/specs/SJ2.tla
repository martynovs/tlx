---- MODULE SJ2 ----
\* Json file natives (Json.java serialize / ndSerialize / deserialize /
\* ndDeserialize): JsonSerialize writes ToJson(value) without a newline,
\* ndJsonSerialize one ToJson line per element; JsonDeserialize and
\* ndJsonDeserialize read arrays as sequences, objects as records, numbers
\* as Gson's getAsInt (truncated, wrapped to 32 bits), skip blank lines.
\* The files live in SJ2-tmp/ (created by the natives' mkdirs, removed at
\* the end).
EXTENDS Json, IOUtils, TLC, Sequences, Integers
Sh(cmd) == IOExec(<<"sh", "-c", cmd>>)
ASSUME Sh("rm -rf SJ2-tmp").exitValue = 0
V == <<1, "a\"b\\c\n\t<>&", TRUE, {3, 1, 2}, [a |-> <<>>, b |-> -7], <<"x">>, {}>>
ASSUME JsonSerialize("SJ2-tmp/v.json", V)
ASSUME Sh("cat SJ2-tmp/v.json").stdout = "[1,\"a\\\"b\\\\c\\n\\t<>&\",true,[1,2,3],{\"a\":[],\"b\":-7},[\"x\"],[]]"
ASSUME JsonDeserialize("SJ2-tmp/v.json") = <<1, "a\"b\\c\n\t<>&", TRUE, <<1, 2, 3>>, [a |-> <<>>, b |-> -7], <<"x">>, <<>>>>
ASSUME ndJsonSerialize("SJ2-tmp/d/v.ndjson", <<1, [a |-> 1], <<"x">>, "s">>)
ASSUME Sh("cat SJ2-tmp/d/v.ndjson").stdout = "1\n{\"a\":1}\n[\"x\"]\n\"s\"\n"
ASSUME ndJsonDeserialize("SJ2-tmp/d/v.ndjson") = <<1, [a |-> 1], <<"x">>, "s">>
\* Numbers: getAsInt; blank lines and \r\n in ndjson; repeated object keys.
ASSUME Sh("printf '[1, -2, 3.7, -3.7, 1e3, 3000000000, 1.5E1, 12e-1, \"\\\\u00e9\"]' > SJ2-tmp/n.json").exitValue = 0
ASSUME JsonDeserialize("SJ2-tmp/n.json") = <<1, -2, 3, -3, 1000, -1294967296, 15, 1, "é">>
ASSUME Sh("printf '\\n  [1]  \\r\\n\\n{\"a\": \"x\", \"a\": \"y\"}\\r\"s\"\\n' > SJ2-tmp/l.ndjson").exitValue = 0
ASSUME ndJsonDeserialize("SJ2-tmp/l.ndjson") = <<<<1>>, [a |-> "y"], "s">>
ASSUME Sh("printf '{}' > SJ2-tmp/e.json").exitValue = 0
ASSUME JsonDeserialize("SJ2-tmp/e.json") = <<>>
\* A function over strings is a record (toRcd).
ASSUME JsonSerialize("SJ2-tmp/f.json", "s" :> 1)
ASSUME Sh("cat SJ2-tmp/f.json").stdout = "{\"s\":1}"
VARIABLE x
Init == x = 0
\* Written during the search (the compiled tier falls back for them).
Next == /\ x < 3
        /\ ndJsonSerialize("SJ2-tmp/s.ndjson", <<x, x + 1>>)
        /\ x' = Len(ndJsonDeserialize("SJ2-tmp/s.ndjson")) + x - 1
Post == Sh("rm -rf SJ2-tmp").exitValue = 0
Spec == Init /\ [][Next]_x
====
