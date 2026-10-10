---- MODULE MC_tlx_Zab_new ----
\* Our own wrapper (not part of upstream).
\* statistics/Zab_new/results.csv lists TLC runs by (#Servers,#Requests,#Crashes,#Partitions).
\* HYPOTHESIZED mapping: Requests -> MaxTransactionNum, Crashes -> MaxRestarts,
\* Partitions -> MaxTimeoutFailures, MaxEpoch unset; Server symmetric. Verify with TLC before trusting
\* the CSV numbers. Two parameter sets below (pick one in the cfg):
\*   small: (3,2,1,0) -> CSV: 394,524 generated / 159,502 distinct / diameter 43
\*   big:   (3,2,2,0) -> CSV: 17,096,433 generated / 7,091,514 distinct / diameter 54 (TLC 20 min)
\*   (CSV also has (3,2,0,2) 99.8M distinct 4h18m, (3,2,2,1) 980M distinct 22h47m)
EXTENDS Zab_new, TLC
CONSTANTS s1, s2, s3, mv_NullPoint
const_Server == {s1, s2, s3}
const_NullPoint == mv_NullPoint
params_small == [MaxTransactionNum |-> 2, MaxRestarts |-> 1, MaxTimeoutFailures |-> 0]
params_big   == [MaxTransactionNum |-> 2, MaxRestarts |-> 2, MaxTimeoutFailures |-> 0]
params_timeout == [MaxTransactionNum |-> 2, MaxRestarts |-> 1, MaxTimeoutFailures |-> 1]
symm_Server == Permutations(const_Server)
====
