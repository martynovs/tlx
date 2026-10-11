# Open issues

This document tracks unresolved work. Fixed defects are removed; their changes and regression
coverage remain in Git history and the relevant design and testing documentation.
[bench/README.md](../bench/README.md#frozen-snapshot-provenance) preserves the initial snapshot's
provenance and historical failures. Fixes and later validation do not rewrite that frozen baseline.

## Full-model validation and performance follow-up

The following checks remain open after the code fixes. Passing a regression test does not establish
completion or performance on the original benchmark configuration.

| Configuration | Remaining work |
|---|---|
| `ext/vanlightly-kafka/kafka_replication_v3_5`, compiled 16 workers and default one worker | Collect repeated same-instance before/after samples with explicit cache conditions. Investigate remaining layout migration and interpreted-region costs; compiler cancellation alone does not establish that the timeout problem is solved. |

Other long-running configurations still need completion or a documented resource bound:
`KeyValueStore/MCKVSSafetyLarge`, `TLC/TestMCReachability`, `byzpaxos/BPConProof`, `c1cs/c1cs`,
`dag-consensus/TLCSailfish2`, `detector_chan96/EnvironmentController`, `diskpaxos/MC_HDiskSynod`
and `ewd998/EWD998`. Several matching TLC runs also timed out; a timeout is not a completed timing.
The [frozen observations](../bench/README.md#timeout-observations-at-the-freeze-point) retain the
original limits and TLC results. Later successful runs, including extended retries, need review
before they become a new baseline.

## Enumeration resource limits

CarTalkPuzzle Model_3 and SpanTreeTest5Nodes no longer fail immediately on cardinality overflow,
but both reached the 600-second warm-up limit with no timed samples in the
[post-fix validation](research/post-fix-validation.md). Their large enumerations still
need optimization or a documented larger resource bound; neither is a completed timing.

## Liveness resource limit

`bcastFolklore/bcastFolklore` exceeded the default memory budget: the frozen run requested about
16,845 MiB against a 15,603 MiB budget. The product graph is large; correcting the separate
MCInnerSerial fairness issue does not remove this resource requirement.

Validate an explicitly larger budget as a separate configuration, or reduce product-graph memory
use. Do not silently raise the budget of an existing measurement or treat a resource failure as
success.

## EWD benchmark input identity and expectations

The correctness drivers now resolve `ewd998/EWD998ChanTrace` from the Examples root
([testing.md](testing.md#61-tlc-oracle-scriptscorpuspy)). The benchmark path and its frozen `error`
expectation have not migrated. The measurement key currently omits the working directory and this
model's trace-data content, so changing staging without changing the identity could reuse evidence
from a different effective input.

The remaining migration must update these together:

- Include the working directory and resolved trace data in the benchmark configuration identity.
- Apply the corrected staging to benchmark execution and collect a new TLC oracle.
- Update the suite expectation and the matching correctness oracle; keep the old measurements
  identifiable as historical evidence. Do not add an expected-failure exemption for this mismatch.

For this trace, use TLC's `-fp 0` when collecting the correctness oracle, as described in
[compat.md](compat.md), D-25. The corrected-input evidence is success with 659 distinct states,
11,169 generated states and depth 655; it does not replace the frozen error record.

The upstream examples `ewd998/EWD998ChanID_export` and `ewd998/EWD998ChanID_shiviz` are incomplete:
`MCInit` leaves `passes` unassigned. TLC and tlx both reject them. They need corrected examples or
explicit classification as invalid inputs, rather than an engine fix.

## Measurement quality and high-speedup validation

Four frozen default-mode configurations have a sample range divided by the median above 25%:
`MultiCarElevator/ElevatorSafetyLarge`, `Prisoners_Single_Switch/PrisonerSolo`,
`SpecifyingSystems/AsynchronousInterface/AsynchInterface` and `barriers/Barrier`.
Keep their samples visible and repeat measurements before using them to establish small changes.

The [audit of ratios above 100x](research/speedup-audit.md) identifies further validation work:
repeated TLC measurements on the same instance, explicit cache conditions, comparable runs with
and without automatic reduction, and per-property correctness checks. `MCParReach` still lacks
phase attribution. Matching counts and a log line announcing liveness checking do not prove that
every temporal property was translated correctly.
