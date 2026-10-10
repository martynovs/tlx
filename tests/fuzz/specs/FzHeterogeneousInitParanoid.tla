---- MODULE FzHeterogeneousInitParanoid ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
Pool == <<"cxac", "aa", "abaxabbcx", "a", "xbcxacac", "c", "aabaxabbcx", "abax", "abxaa", "xx", "bcxacac", "ac", "xb", "b", "cxacac", "babab", "abaxabbc", "aaab", "abxaab", "xab", "xxx", "abc", "bab", "caa", "abxc", "cx", "abb", "bx", "x", "", "abcbab", "ab", "abbc", "bc", "cb", "ca", "cc", "aba", "abx", "aab", "xa", "abab">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
VARIABLES pc, v_0_0, r_0
E_0 == {[a |-> 0, b |-> (-2)], v_0_0}
X_0 == 0
Init ==
  /\ pc = 0
  /\ v_0_0 = {}
  /\ r_0 = E_0
Step == pc = 0 /\ pc' = 1 /\ UNCHANGED <<v_0_0, r_0>>
Next == Step
Inv_0 == r_0 = X_0
====
