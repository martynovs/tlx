---- MODULE CMP73A ----
\* 0-indexed arrays as records of sequences (the braf idiom): action
\* properties quantify over `[elems: [1..len -> S]]` for a constant range
\* of `len` (unrolled by the compiled tier, the elements then have the
\* shape of the state variables), and a conditional builds an array in
\* the encoding of the variable in its other branch. One property fails.
EXTENDS Naturals, Sequences
VARIABLES buf, file
S == {"a", "b"}
Max(a, b) == IF a <= b THEN b ELSE a
Array(T, len) == [elems: [1..len -> T]]
ArrayLen(a) == Len(a.elems)
ArrayGet(a, i) == a.elems[i + 1]
MkArray(len, f) == [elems |-> [i \in 1..len |-> f[i - 1]]]
ArraySlice(a, s, e) == [elems |-> SubSeq(a.elems, s + 1, e)]
Overlay(f, off, d) ==
    LET n == Max(ArrayLen(f), off + ArrayLen(d)) IN
    MkArray(n, [i \in 0..(n - 1) |->
        IF i >= off /\ i < off + ArrayLen(d) THEN ArrayGet(d, i - off)
        ELSE IF i < ArrayLen(f) THEN ArrayGet(f, i) ELSE "z"])
Content == IF ArrayLen(buf) > 0 THEN Overlay(file, 0, buf) ELSE file

Init == buf = [elems |-> <<>>] /\ file = [elems |-> <<>>]
Put(d) == /\ ArrayLen(buf) + ArrayLen(d) <= 3
          /\ buf' = [elems |-> buf.elems \o d.elems]
          /\ UNCHANGED file
Flush == /\ ArrayLen(buf) > 0
         /\ file' = Content
         /\ buf' = [elems |-> <<>>]
Next == \/ \E len \in 1..2 : \E d \in Array(S, len) : Put(d)
        \/ Flush

PutCorrect == [][\A len \in 1..2 : \A d \in Array(S, len) :
                    Put(d) => \E k \in 1..len : ArraySlice(buf', ArrayLen(buf), ArrayLen(buf) + k) = ArraySlice(d, 0, k)]_<<buf, file>>
FlushKeeps == [][Flush => file' = Content /\ ArrayLen(file') = Max(ArrayLen(file), ArrayLen(buf))]_<<buf, file>>
\* Fails: a two-symbol write when the buffer already holds one symbol.
Short == [][\A len \in 1..2 : \A d \in Array(S, len) : Put(d) => ArrayLen(buf') <= 2 \/ ArrayGet(d, 0) = "b"]_<<buf, file>>
TypeOK == /\ buf \in [elems: Seq(S)] /\ ArrayLen(buf) <= 3
          /\ file \in [elems: Seq(S \cup {"z"})] /\ ArrayLen(file) <= 3
====
