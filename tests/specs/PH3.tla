---- MODULE PH3 ----
\* Statistics (Statistics.java): ChiSquare runs Apache Commons Math 3.6.1's
\* ChiSquareTest, returning FALSE when the null hypothesis is rejected at the
\* given significance level (asserted in a POSTCONDITION). tlx reproduces the
\* p-value (regularized gamma, Lanczos logGamma) at the same precision, so the
\* decision agrees with TLC here too, on exact critical values (p = alpha) and
\* on the rescaling path (the observed and expected totals differ).
EXTENDS Statistics, TLC, Sequences, FiniteSets, Naturals, Integers
ASSUME Print("equal:", ChiSquare([i \in 1..5 |-> 6], [i \in 1..5 |-> 6], "0.2")) = TRUE
ASSUME Print("chi:", ChiSquare(<<100, 100>>, <<105, 95>>, "0.5")) = FALSE
ASSUME Print("chi:", ChiSquare(<<100, 100>>, <<114, 86>>, "0.05")) = FALSE
ASSUME Print("chi:", ChiSquare(<<100, 100>>, <<118, 82>>, "0.01")) = TRUE
ASSUME Print("chi:", ChiSquare(<<100000, 100000>>, <<100151, 99849>>, "0.5")) = FALSE
ASSUME Print("chi:", ChiSquare(<<100000, 100000>>, <<100438, 99562>>, "0.05")) = TRUE
ASSUME Print("chi:", ChiSquare(<<100000, 100000>>, <<100576, 99424>>, "0.01")) = FALSE
ASSUME Print("chi:", ChiSquare(<<100000000, 100000000>>, <<100004769, 99995231>>, "0.5")) = TRUE
ASSUME Print("chi:", ChiSquare(<<100000000, 100000000>>, <<100013859, 99986141>>, "0.05")) = TRUE
ASSUME Print("chi:", ChiSquare(<<100000000, 100000000>>, <<100018214, 99981786>>, "0.01")) = FALSE
ASSUME Print("chi:", ChiSquare(<<1000000, 1000000, 1000000, 1000000, 1000000, 1000000>>, <<1001102, 998898, 1001102, 998898, 1001102, 998898>>, "0.2")) = TRUE
ASSUME Print("chi:", ChiSquare(<<1000000, 1000000, 1000000, 1000000, 1000000, 1000000>>, <<1001358, 998642, 1001358, 998642, 1001358, 998642>>, "0.05")) = TRUE
ASSUME Print("chi:", ChiSquare(<<631, 170>>, <<408, 187>>, "0.01")) = FALSE
ASSUME Print("chi:", ChiSquare(<<695, 759, 599, 799, 279>>, <<916, 657, 434, 553, 87>>, "0.2")) = FALSE
ASSUME Print("chi:", ChiSquare(<<377, 727, 298, 267>>, <<171, 406, 368, 1225>>, "0.05")) = FALSE
ASSUME Print("chi:", ChiSquare(<<240, 868, 44, 865, 452>>, <<508, 363, 462, 349, 1815>>, "0.2")) = FALSE
ASSUME Print("chi:", ChiSquare(<<613, 815>>, <<1917, 1910>>, "0.01")) = FALSE
VARIABLE x
Init == x = 0
Next == x' = x
Spec == Init /\ [][Next]_x
====
