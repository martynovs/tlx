---- MODULE FzHeterogeneousSetMapParanoid ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
Pool == <<"aaabxbc", "bbxa", "aa", "xxx", "bxabb", "abbxxb", "abababa", "bab", "ccaax", "xx", "xc", "abbba", "xxax", "xxxb", "abbxxa", "axx", "bababc", "cababac", "aabba", "bxa", "abbbbcba", "cxcc", "aaxb", "bbxx", "xab", "aaa", "aabx", "bcac", "cxxxa", "xaabbbbx", "ccx", "bcba", "abababc", "bc", "ccab", "cx", "ababa", "aba", "babb", "bba", "acab", "xxb", "abababxx", "cabbbc", "ccaabababc", "acba", "aaxbabac", "caabxx", "xxxab", "b", "abbxccaabababc", "bbcxbabccbbx", "bxxabbx", "aab", "bxcabb", "x", "cababa", "ccbb", "ax", "abxb", "bxbcac", "acbaab", "axab", "abaab", "xabcccabxabab", "bxxb", "bbc", "xca", "xbaba", "cxbabx", "bbcxbab", "bxxx", "ababxxbbaabax", "caabx", "xxab", "ccaaxbxc", "xaxab", "bbabx", "babcxacbaab", "abxx", "cab", "xabc", "aabax", "abab", "axac", "xb", "caaab", "caa", "xa", "abbbc", "xaaa", "bxc", "bbxxb", "cabb", "caaba", "abbbbcbacaaba", "bxx", "acc", "abac", "cac", "abx", "bxbc", "cxa", "cca", "xcc", "abbx", "ccxccaaabxaabbbbx", "abcx", "axxacab", "bbcx", "abax", "bbabxccab", "aabbbbx", "abccx", "ccac", "xabxabcababac", "aabab", "bbx", "cxb", "xaxaba", "", "ababxx", "abbb", "bxxa", "abc", "ccbbx", "abxabab", "cxbxabxb", "ab", "bx", "babcx", "axxb", "xcabxaa", "baa", "xabccc", "abaa", "xxxa", "bxcc", "xba", "xbx", "xbxabxb", "abababaa", "a", "axb", "xcaabc", "abaabbxabxcc", "xabxab", "xcabx", "axabab", "aabxbb", "babbaab", "xccx", "xababbxxa", "abb", "babx", "abaabbxa", "ccxc", "babbxc", "xxccx", "c", "aabb", "cc", "bcaabxx", "cxab", "cbxabb", "xbxx", "ac", "xabxabcababacbabcxacbaab", "abbc", "aabxbbbcbcaabxx", "xxcaabc", "xabxxx", "bcab", "abcab", "bbaabax", "cb", "bb", "aabxbbbc", "ca", "babaa", "xxa", "baba", "abababx", "cbx", "xbax", "ba", "xbaccac", "xccabbc", "abaababb", "ccxccaaab">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
VARIABLES pc, v_0_0, r_0
E_0 == {v_0_0[h] : h \in DOMAIN v_0_0}
X_0 == 0
Init ==
  /\ pc = 0
  /\ v_0_0 = (1 :> "b" @@ 2 :> {0})
  /\ r_0 = 0
Step == pc = 0 /\ pc' = 1 /\ UNCHANGED <<v_0_0, r_0>>
Next == Step
Inv_0 == E_0 = E_0
====
