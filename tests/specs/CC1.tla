---- MODULE CC1 ----
\* Contract: a cfg value assignment that names neither a constant nor a
\* definition of the root module's scope is silently ignored, not an error:
\* an undeclared name, a variable, a definition that only exists inside an
\* INSTANCEd module, or a module-scoped form naming an unknown module (TLC:
\* SpecProcessor.processConfig only assigns what it finds in the root
\* module's tables; CC1.cfg also assigns `N`, so the model is accepted).
CONSTANT N
VARIABLE x
I == INSTANCE CC1Inner WITH N <- 5
Init == x = I!Foo
Next == x' = x
Inv == x = 5
====
