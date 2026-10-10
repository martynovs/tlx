# Runtime symmetry obligations

Run `python3 scripts/autosym_mutants.py --guard-fixtures` for the bounded
projection fixtures in interpreter, closure IR, C, JIT and paranoid modes.
`--refresh` refreshes the small TLC oracle with manually declared symmetry.

The PaxosCommit configuration reduces the Examples `transaction_commit`
model to one resource manager and two acceptors. The two VR configurations
come from `vanlightly-vsr/analysis/03-state-transfer/VR_STATE_TRANSFER.cfg`
and `analysis/04-application-state/VR_APP_STATE.cfg`; they use two replicas
and one timer-triggered view change. Their VIEW is omitted because automatic
symmetry does not apply to VIEW models. The driver copies the original
modules from `third_party` into an owned temporary directory: it does not
modify the downloaded corpus or maintain duplicate specification sources.

`GuardLate` agrees through the bounded probe, then gives different projected
values. Its runtime guard must restart without automatic symmetry and report
the same violation, counters and trace length as the unreduced run.
