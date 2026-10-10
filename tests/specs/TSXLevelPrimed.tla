---- MODULE TSXLevelPrimed ----
\* TLCGet("level") primed is the level of the next state, one more than
\* the current one: in the next-state relation, an action constraint and
\* an action property (TLC's TLCGetLevelTest).
EXTENDS Integers, TLC
VARIABLES x, z

Init == x = 0 /\ z = TLCGet("level")

Next ==
   /\ TLCGet("level") > 0
   /\ Assert(TLCGet("level") + 1 = TLCGet("level")', "Failure pre next-state")
   /\ x < 3
   /\ x' = x + 1
   /\ z' = TLCGet("level")'
   /\ Assert(TLCGet("level") + 1 = TLCGet("level")', "Failure post next-state")

Inv == x + 1 = TLCGet("level") /\ (x > 0 => z = TLCGet("level"))

ActionConstraint == Assert(TLCGet("level") + 1 = TLCGet("level")', "Failure action constraint")

Prop == [][TLCGet("level") + 1 = TLCGet("level")']_<<x, z>>
====
