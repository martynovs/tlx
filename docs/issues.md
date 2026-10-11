# Initial benchmark baseline and open issues

The initial baseline is frozen at **2026-10-11 08:51:23 +08:00**. Later cloud results do not
automatically change it. [performance.md](performance.md) is generated from this frozen store;
[benchmarking.md](benchmarking.md) defines the measurement protocol.

## Scope and provenance

The suite contains **235 models and 299 configurations**. At the freeze point:

| State | Configurations | Treatment |
|---|---:|---|
| Three complete timed samples, with no configured expectation failure | 279 | Eligible for performance tables |
| Previously timed out; longer retries not all complete | 12 | No headline timing |
| Execution error | 6 | No headline timing |
| Result differs from a fixed expectation | 2 | No headline timing |

The complete configurations include 247 successes, 16 safety failures, 12 deadlocks and four
liveness failures. A valid counterexample is a completed check. However, 39 complete configurations
have no pinned expectation; completion is not a claim that every case has full TLC validation.
The default-mode contract permits different state counts; the compatible compiled runs check
their applicable counts as well as verdicts.

There are 300 canonical tlx records covering these 299 configurations: the unfinished Slush retry
also has a partial, unkeyed record. It is excluded by the repetition guard and has no TLC pairing.
Only the new measured revision is retained in the active tlx store. The 470 TLC records for current input/version/machine keys remain; other versions and historical
keys are archived. Performance ratios use complete matching keys only.

Batches 1–3 have final cloud receipts. Batches 4–5 contribute explicitly frozen **in-progress
checkpoints**, not final passing batch verdicts. Their checked-out revision, clean tracked tree
and measured binary SHA-256 were checked when freezing them. The snapshot's `bench/environment/`
holds machine descriptions, source checks, sampling configurations and `outcomes.tsv`.

The source-equivalent initial commit is found with `git rev-list --max-parents=0 HEAD`.
The measurement files retain the original collection revision; these identities are not
rewritten when documentation or Git history changes.
The Rust workspace remains identical; these fingerprints identify it independently of that amend:

| Input | Identity |
|---|---|
| Git tree for `crates/` | `90c32255b973c17ea2a12c3b94feadff3f3c3d5b` |
| `Cargo.toml` SHA-256 | `0bf5e41720543745c8c53483e5f2b8f503ec08e877642a425b13c9acc6fa9641` |
| `Cargo.lock` SHA-256 | `a3accb8f37e52206e92365dd61f741ba0122a0e667bbb106d5a307e9a2afab2f` |
| `rust-toolchain.toml` SHA-256 | `62774a7ceb3920b35db9f0632ef324fb941ee3bad9a9acb949e4c5bbd32fdede` |

The current canonical records (including embedded original per-configuration samples) and
environment evidence ship in Git, so a fresh clone can reproduce the report. Raw suite histories
are archived under `work/`; active records point to their own canonical measurement objects. Original collection locators are preserved as metadata
on independent canonical baseline records. Superseded records, exact source bundles and optional
backup snapshots are retained under the maintainer's ignored `work/` archive, as described in
[bench/README.md](../bench/README.md).

## Correctness difference requiring investigation

`ext/ccf/MCccfraft_1C3N`, compiled with TLC compatibility at both one and 16 workers, reports
`TypeInv` as violated at depth 3. The suite expects success. The existing TLC baselines with the
same input digest and TLC version complete successfully, with 675,893 distinct states and depth 66.

Input digest: `703571406163bcea5847e8c56e7925e794c11f6741e5a8eeb6aece81c2c0c913`.

Both configurations are excluded from performance claims. Their early termination is not a
speedup. The cause was found and fixed; the suite expectation was never changed to make the
run pass. `UNION {f(x) : x \in S}` (`UnionFold`) was lowered to a word-wise or of the encoded
members, which is the set union only for a bitset. The set of in-flight messages is a set of
records whose element universe (three record arms, about 1.8e4 values) is far above the bitset
universe (4,096), so it is a sorted array: or-ing the encodings of `UNION {Range(messages[s])}`
produced a different record, and the first invariant reading a field of such an element
(`TypeInv`, through `MessagesTypeInv`) reported a violation on a legal state.
`crates/tlx-compile/src/lower/sets.rs` now keeps the fold for bitset members only and leaves the
other members to the interpreter. The regression is
`crates/tlx-compile/tests/union_of_ranges.rs`: it fails before the change and covers both
directions (a union over a sorted-array set agrees with the interpreter, an atoms-only union
keeps the fold and stays native). The recorded early termination stands as the state of the
frozen measurement; the configuration needs a fresh compiled run before it can be timed, and
the TLC baseline above is unchanged.

## Execution errors and incomplete diagnostic observations

| Model | Observation | Follow-up |
|---|---|---|
| `CarTalkPuzzle/CarTalkPuzzle.toolbox/Model_3/MC` | Cardinality overflow while evaluating an assumption | Review large-set cardinality evaluation and the model's assumptions |
| `SpanningTree/SpanTreeTest5Nodes` | Cardinality overflow for nested `SUBSET` | Review cardinality handling |
| `bcastFolklore/bcastFolklore` | Liveness checking needs about 16,845 MiB, beyond the default 15,603 MiB budget | Measure an explicitly larger budget separately; do not silently change this configuration |
| `ewd998/EWD998ChanID_export` | Initial state is not legal | Check the export example's setup and state construction |
| `ewd998/EWD998ChanID_shiviz` | Initial state is not legal | Check the export example's setup and state construction |
| `ewd998/EWD998ChanTrace` | Cannot find `specifications/ewd998/EWD998ChanTrace.ndjson` at runtime | Correct trace-data staging/path resolution; this suite entry already expects `error` |

An additional observation arrived during the unfinished retry of
`SpecifyingSystems/AdvancedExamples/MCInnerSerial`: its warm-up returned a liveness-memory-budget
error after 829.443 seconds. Safety exploration had reached 195 distinct states and depth 5,
matching the stored TLC safety counts. This is a diagnostic from an unfinished configuration,
not a new complete timing or a final verdict for that retry. It should be investigated alongside
the liveness memory accounting and budget behavior.

## The 12 configurations with earlier timeouts

All used a 600-second bound in the earlier attempts. Extended retries use a single-sample bound
of 1,800 seconds. At the freeze point, the two Kafka configurations and `MCKVSSafetyLarge` had also
hit that longer bound. The other nine retries were not complete. Slush had a successful warm-up
and two successful timed samples (900.949 and 897.232 seconds); two samples do not satisfy the
three-sample baseline requirement.

These TLC records match the input digest, TLC version, SA5.4XLARGE32/x86_64 instance class and
worker count. Timeout durations are lower bounds, not completed TLC timings.

| Model | Workers | TLC seconds | TLC result |
|---|---:|---:|---|
| `ext/vanlightly-kafka/kafka_replication_v3_5` (compiled tlx) | 16 | 52.676 | Deadlock |
| `ext/vanlightly-kafka/kafka_replication_v3_5` (default tlx) | 1 | 755.553 | Deadlock |
| `KeyValueStore/MCKVSSafetyLarge` | 1 | 7,200.188 | Timeout |
| `SlushProtocol/SlushLarge` | 1 | 10,794.133 | Success |
| `SpecifyingSystems/AdvancedExamples/MCInnerSerial` | 1 | 462.895 | Success |
| `TLC/TestMCReachability` | 1 | 1,492.239 | Liveness failure |
| `byzpaxos/BPConProof` | 1 | 3,600.256 | Timeout |
| `c1cs/c1cs` | 1 | 7,200.243 | Timeout |
| `dag-consensus/TLCSailfish2` | 1 | 7,200.133 | Timeout |
| `detector_chan96/EnvironmentController` | 1 | 7,200.090 | Timeout |
| `diskpaxos/MC_HDiskSynod` | 1 | 7,200.293 | Timeout |
| `ewd998/EWD998` | 1 | 7,200.309 | Timeout |

Seven TLC configurations also failed to finish within one or two hours. Kafka is a different
case: native C compilation consumed much of the compiled tlx warm-up. Compile/startup cost and
warmed search cost need separate investigation; no ratio is calculated from an unfinished warm-up.

## Measurement quality and collection fixes

Four complete default-mode configurations have a timed-sample range divided by the median above
25%: `MultiCarElevator/ElevatorSafetyLarge` (30.8%), `Prisoners_Single_Switch/PrisonerSolo` (26.2%),
`SpecifyingSystems/AsynchronousInterface/AsynchInterface` (40.0%) and `barriers/Barrier` (29.3%).
Keep their samples visible; do not use them to establish small changes in performance.

The collection found two reporting/process issues, corrected in the initial release's tooling:

- Canonical normalization discarded the suite's `count_problems`. It now preserves those
  diagnostics, excludes mismatches from performance evidence and rejects mismatched TLC pairings.
- A checker timeout could leave native compiler descendants running. Benchmark capture now owns
  an explicit process group and cleans it on timeout/cancellation and scope exit. The observed
  leftover compiler was stopped during the next configuration's warm-up, before timed samples.

These are tooling fixes, not fixes for the model-checking differences above. Frozen measurements
retain their original source identity and sample completeness; later retries require a separately
reviewed update to the baseline.
