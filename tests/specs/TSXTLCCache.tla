---- MODULE TSXTLCCache ----
\* TLCExt!TLCCache(e, closure): a constant-level e is evaluated once per
\* closure value (TLCExt.java tlcEval2), wherever it is referenced (TLC's
\* ConstantContextTLCCacheTest). A register counts the evaluations; e that
\* is not constant-level is just evaluated.
EXTENDS Integers, TLC, TLCExt

COUNT_ID == 1
ASSUME TLCSet(COUNT_ID, 0)

inc(v) == CASE TLCSet(COUNT_ID, TLCGet(COUNT_ID) + 1) -> v

defn == TLCCache(inc({ x : x \in 1 .. 5 }), {1})
derivedDefn == TLCCache(inc({ i \in 1 .. 5 : i + 1 \in defn }), {2})
\* Another closure value: evaluated again.
other == TLCCache(inc({ x : x \in 1 .. 5 }), {3})

VARIABLE x, y

ASSUME 5 \in defn
ASSUME 5 \notin derivedDefn

Init ==
    /\ y = defn
    /\ x = derivedDefn

Next ==
    /\ 1 \in defn
    /\ 1 \in derivedDefn
    /\ Assert(TLCGet(COUNT_ID) = 2, <<"count was", TLCGet(COUNT_ID)>>)
    /\ UNCHANGED <<x, y>>

\* Not constant-level: no caching (TLCCache of a state expression).
Inv == TLCCache(x \cup y, {}) = y
====
