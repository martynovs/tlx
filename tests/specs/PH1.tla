---- MODULE PH1 ----
\* Json (Json.java): ToJson / ToJsonArray / ToJsonObject convert a value to
\* a Gson tree and return its toString — no whitespace, sets and sequences
\* as arrays, records and non-sequence functions as objects, and Gson's
\* escaping (no HTML escaping, so < > & ' stay literal).
EXTENDS Json, TLC, Sequences, FiniteSets, Naturals, Integers
CONSTANT MV
\* A sequence is a JSON array.
ASSUME Print("tuple:", ToJson(<<1, "a">>)) = "[1,\"a\"]"
\* A record is a JSON object (fields in their stored order).
ASSUME Print("record:", ToJson([a |-> 1, b |-> 2])) = "{\"a\":1,\"b\":2}"
\* A set is an array in normalized (sorted) order.
ASSUME Print("set:", ToJson({1, 2, 3})) = "[1,2,3]"
ASSUME Print("emptyset:", ToJson({})) = "[]"
ASSUME Print("emptyseq:", ToJson(<<>>)) = "[]"
\* A function whose domain is 1..n is a sequence: an array.
ASSUME Print("fcn:", ToJson([i \in 1..3 |-> i * 2])) = "[2,4,6]"
\* ...but any other domain makes it an object keyed by `toString`.
ASSUME Print("fcnoffset:", ToJson([i \in 2..3 |-> i * 2])) = "{\"2\":4,\"3\":6}"
\* Scalars.
ASSUME Print("bool:", ToJson(TRUE)) = "true"
ASSUME Print("int:", ToJson(42)) = "42"
ASSUME Print("neg:", ToJson(-7)) = "-7"
ASSUME Print("str:", ToJson("hi")) = "\"hi\""
ASSUME Print("modelvalue:", ToJson(MV)) = "\"mv\""
\* Escaping: quote, backslash, newline, tab; no HTML escaping.
ASSUME Print("escape:", ToJson("a\"b\\c\nd\te")) = "\"a\\\"b\\\\c\\nd\\te\""
ASSUME Print("htmlsafe:", ToJson("<>&='")) = "\"<>&='\""
\* Nesting, and a set inside a record.
ASSUME Print("nested:", ToJson([a |-> {2, 1}])) = "{\"a\":[1,2]}"
ASSUME Print("seqrec:", ToJson(<<[a |-> 1]>>)) = "[{\"a\":1}]"
\* ToJsonArray: sequences and sets become arrays.
ASSUME Print("arrtuple:", ToJsonArray(<<1, <<2, 3>>>>)) = "[1,[2,3]]"
ASSUME Print("arrset:", ToJsonArray({3, 1, 2})) = "[1,2,3]"
\* ToJsonObject: records become objects, sequences become objects keyed by
\* the index.
ASSUME Print("objrec:", ToJsonObject([a |-> 1, b |-> 2])) = "{\"a\":1,\"b\":2}"
ASSUME Print("objtuple:", ToJsonObject(<<10, 20>>)) = "{\"0\":10,\"1\":20}"
VARIABLE x
Init == x = 0
Next == x' = x
Spec == Init /\ [][Next]_x
====
