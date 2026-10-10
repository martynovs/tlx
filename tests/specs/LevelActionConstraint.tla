---- MODULE LevelActionConstraint ----
\* TLCGet("level") in an ACTION_CONSTRAINT is the level of the state the
\* step leaves (TLC's s0.getLevel()), and in the VIEW the level of the state;
\* stuttering steps make new states because the VIEW has the level (the
\* idiom of EWD998ChanTrace).
EXTENDS Naturals, TLC
VARIABLE x
Init == x = 0
Next == \/ x < 2 /\ x' = x + 1
        \/ UNCHANGED x
Spec == Init /\ [][Next]_x
View == <<x, TLCGet("level")>>
AC == TLCGet("level") < 4
====
