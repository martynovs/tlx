---- MODULE TSFPropForall ----
\* Contract: TLC splits a bounded `\A k \in S : F` in a PROPERTY into F for
\* each k: `[]P` parts are invariants and `[][A]_v` parts action properties
\* checked during the search (named by the location of F), not a liveness
\* property checked afterwards; the state space here is infinite.
EXTENDS Naturals
VARIABLE s
Init == s = 0
Next == s' = s + 1
Spec == Init /\ [][Next]_s
Prop == \A k \in {6, 9} : [][s' # k]_s /\ <>(s > k)
====
