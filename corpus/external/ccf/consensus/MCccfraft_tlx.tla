---------- MODULE MCccfraft_tlx ----------
\* tlx corpus (ours): MCccfraft reads its parameters from environment variables
\* (IOEnv RAFT_CONFIGS, TERM_COUNT, REQUEST_COUNT, DISABLE_CHECK_QUORUM; CCF's tlc.py
\* sets them). The corpus scripts pass no environment, so the cfg files
\* MCccfraft_tlx_*.cfg override those definitions with these fixed values (the
\* runs of CCF's ci-verification.yml).
EXTENDS MCccfraft

tlx_Configs_1C2N == <<{NodeOne, NodeTwo}>>
tlx_Configs_1C3N == <<{NodeOne, NodeTwo, NodeThree}>>
tlx_Configs_2C2N == <<{NodeOne}, {NodeTwo}>>
tlx_Count0 == 0
tlx_Count2 == 2
tlx_Count3 == 3
===================================
