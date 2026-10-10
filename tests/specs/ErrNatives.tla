---- MODULE ErrNatives ----
\* The natives IOUtils!atoi (Java's Integer.parseInt) and
\* Functions!IsInjective as TLC's overrides, and TLCExt!AssertError where it
\* is FALSE. (Where AssertError is TRUE, the message comparison only holds
\* without -tool, which this test runs TLC with: see
\* crates/tlx-cli/tests/java_only.rs.)
EXTENDS Naturals, Sequences, TLC, TLCExt, Functions, IOUtils

\* No error, or another message: FALSE.
ASSUME ~AssertError("anything", 1 + 1)
ASSUME ~AssertError("another message", atoi("x"))
ASSUME ~AssertError("The argument of atoi", atoi("x"))

\* atoi is Java's Integer.parseInt.
ASSUME atoi("-0") = 0 /\ atoi("+7") = 7 /\ atoi("-12") = -12 /\ atoi("007") = 7
ASSUME atoi("2147483647") = 2147483647 /\ atoi("-2147483648") = -2147483647 - 1

\* IsInjective, also of a set of records (TLC: its field sets).
ASSUME IsInjective(<<1, 2>>) /\ ~IsInjective(<<1, 1>>) /\ IsInjective(<<>>)
ASSUME IsInjective([i \in 1..10 |-> {i}]) /\ ~IsInjective([i \in 1..10 |-> {1, 2}])
ASSUME IsInjective("a" :> 1 @@ "b" :> 2) /\ ~IsInjective("a" :> 1 @@ "b" :> 1)
ASSUME IsInjective([a : {1}, b : {2}]) /\ ~IsInjective([a : {1}, b : {1}])
ASSUME IsInjective([a : 1, b : 2]) /\ ~IsInjective([a : 1, b : 1])

VARIABLE x
Init == x = 0
Next == x < 3 /\ x' = x + 1
Inv == IsInjective(<<x, atoi("5")>>) = (x # 5)
====
