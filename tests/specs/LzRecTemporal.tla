---- MODULE LzRecTemporal ----
\* Recursive operators with a temporal body whose recursion ends at an IF
\* guard that is constant: only the chosen branch is translated
\* (tlaplus/tlaplus#1389), so the property is a finite formula.
EXTENDS Naturals
VARIABLE x
RECURSIVE ap_0(_)
ap_0(u) == IF u THEN <>(x = TRUE) ELSE ap_0(~u)
RECURSIVE ap_2(_)
ap_2(n) == IF n = 0 THEN <>(x = TRUE) ELSE ap_2(n - 1)
RECURSIVE ap_3(_)
ap_3(n) == IF n > 0 THEN ap_3(n - 1) ELSE <>(x = TRUE)
RECURSIVE never(_)
never(n) == IF n = 0 THEN []<>(x = FALSE) /\ <>[](x = TRUE) ELSE never(n - 1)
Spec == x = TRUE /\ [][x' = ~x]_x /\ WF_x(x' = ~x)
PropIf == ap_0(FALSE)
PropDepth == ap_2(3)
PropElseRec == ap_3(3)
PropViolated == never(2)
====
