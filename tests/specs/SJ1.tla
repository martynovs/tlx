---- MODULE SJ1 ----
\* MapThenFoldSet with the `LAMBDA s : CHOOSE x \in s : TRUE` chooser
\* (MapThenSumSet, user folds) takes the elements in TLC's normalized order;
\* tlx evaluates that case in linear time. The general chooser (`x = x`, a
\* user-defined order) must give the same results.
EXTENDS FiniteSetsExt, Folds, Integers, Sequences, TLC
First(s) == CHOOSE x \in s : TRUE
Some(s) == CHOOSE x \in s : x = x
Max2(s) == CHOOSE x \in s : \A y \in s : y <= x
Cat(a, b) == <<a>> \o b
S1 == {"b", "a", "cc", "d"}
S2 == {<<2>>, <<1, 1>>, <<>>, <<3>>}
S3 == {{2}, {1}, {}, {1, 2}}
S4 == {[a |-> 2], [a |-> 1], [a |-> 0]}
ASSUME Print("lambda:", MapThenFoldSet(Cat, <<>>, LAMBDA x : x, LAMBDA s : CHOOSE x \in s : TRUE, S1)) = MapThenFoldSet(Cat, <<>>, LAMBDA x : x, Some, S1)
ASSUME Print("def:", MapThenFoldSet(Cat, <<>>, LAMBDA x : x, First, S2)) = MapThenFoldSet(Cat, <<>>, LAMBDA x : x, Some, S2)
ASSUME Print("sets:", MapThenFoldSet(Cat, <<>>, LAMBDA x : x, First, S3)) = MapThenFoldSet(Cat, <<>>, LAMBDA x : x, Some, S3)
ASSUME Print("records:", MapThenFoldSet(Cat, <<>>, LAMBDA x : x, First, S4)) = MapThenFoldSet(Cat, <<>>, LAMBDA x : x, Some, S4)
ASSUME Print("max:", MapThenFoldSet(Cat, <<>>, LAMBDA x : -x, Max2, {3, -1, 7, 0}))
         = <<-7, -3, 0, 1>>
ASSUME Print("first:", MapThenFoldSet(Cat, <<>>, LAMBDA x : 10 * x, First, {3, -1, 7, 0}))
         = <<-10, 0, 30, 70>>
ASSUME Print("sum:", MapThenSumSet(LAMBDA x : x * x, 1..1000)) = 333833500
ASSUME Print("empty:", MapThenFoldSet(Cat, <<9>>, LAMBDA x : x, First, {})) = <<9>>
VARIABLE x
Init == x = MapThenSumSet(LAMBDA y : y, 1..3)
Next == x' = MapThenFoldSet(LAMBDA a, b : (a + b) % 7, x, LAMBDA y : y, First, {x, x + 1})
Spec == Init /\ [][Next]_x
====
