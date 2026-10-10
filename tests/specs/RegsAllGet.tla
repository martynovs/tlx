---- MODULE RegsAllGet ----
\* TLCGet("all") and TLCGet("all:named"): per register of worker 0, the
\* tuple of the values of all workers (TLC's AbstractChecker.getAllValues
\* and getAllNamedRegisterValues). Integer keys exclude named registers and
\* vice versa; named keys keep their "s:" prefix.
EXTENDS Naturals, TLC, Sequences

ASSUME TLCSet(3, 0)
ASSUME TLCSet("s:count", 0)
ASSUME TLCSet("s:other", "a")
ASSUME DOMAIN TLCGet("all") = {3} /\ TLCGet("all")[3][1] = 0

VARIABLE x

Init == x = 0

Next ==
    /\ x < 5
    /\ TLCSet(3, TLCGet(3) + 1)
    /\ TLCSet("s:count", TLCGet("s:count") + x)
    /\ x' = x + 1

Sum(t) == LET S[i \in 0..Len(t)] == IF i = 0 THEN 0 ELSE t[i] + S[i - 1] IN S[Len(t)]

Post ==
    LET all == TLCGet("all")
        named == TLCGet("all:named")
    IN /\ DOMAIN all = {3}
       /\ Len(all[3]) = TLCGet("config").worker
       /\ Sum(all[3]) = 5
       /\ DOMAIN named = {"s:count", "s:other"}
       /\ Sum(named["s:count"]) = 10
       /\ named["s:other"][1] = "a"
       /\ PrintT(<<all, named>>)
====
