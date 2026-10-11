# Current benchmark baseline

`bench/` contains the current benchmark inputs and evidence and is tracked in Git.
Superseded measurements, source bundles, packaged snapshots and one-off analysis belong under
`work/`, which is ignored. Do not use this directory as an archive.

| Path | Purpose |
|---|---|
| `suite.json` | Model selection, modes, workers, limits and expected results |
| `repr-models.toml`, `repr-configs/` | Reduced model configurations and wrappers |
| `stress/`, `scaling/` | Fixed stress-model configurations and wrappers |
| `report-policy.json` | Pinned report reference, input keys and freeze note |
| `records/` | Current canonical tlx measurements, including excluded/incomplete evidence |
| `tlc-baselines/` | TLC records matching the current input/version/machine keys |
| `environment/` | Machine descriptions, sampling configurations, source checks and outcomes |
| `validation/` | Subsequent fix-validation evidence, kept separate from the frozen baseline |

The initial baseline was frozen on 2026-10-11 at 08:51:23 +08:00. The source-equivalent initial commit is found with
`git rev-list --max-parents=0 HEAD`. Measurements retain their original source identity in the
evidence files; history cleanup does not relabel them.
The Rust workspace is unchanged by the documentation and benchmark-tool updates that finalize the
initial commit. The frozen snapshot below records its source fingerprints and incomplete configurations.
[Open issues](../docs/issues.md) tracks remaining work, rather than the state of past runs.

## Validate and reproduce

A fresh clone can validate inputs and reproduce the report without running models or fetching
private measurement archives:

```sh
PYTHONPATH=scripts python3 -m benchmarking.snapshot inputs
python3 scripts/bench.py report --check
python3 scripts/bench.py report
```

Canonical files retain their samples and embedded per-configuration results as independent
evidence. Original collection locators remain metadata; raw runner histories are archived under
`work/` and are not required to reproduce or validate the report. Superseded
versions and records for other inputs or machines are archived outside this directory.

The report states the measurement conditions it depends on: a tlx time is the median of the timed
samples, each a separate process run after a discarded warm-up, and the warm-up is not a cold-cache
measurement (the native cache may be reused across the batch rather than isolated per configuration).
A row carries the optimization its recorded samples name in the `tlx optimizations` column
(`none` only when every sample records an empty list, `unknown` when the record does not state it),
so a row whose tlx run was reduced says so without claiming what the TLC baseline was configured with.

To run new measurements, fetch dependencies using `sh scripts/fetch-deps.sh` and
`python3 scripts/fetch-external.py`, then resolve the suite with
`python3 scripts/bench.py suite-check`. [benchmarking.md](../docs/benchmarking.md) defines the
measurement protocol and machine requirements. Input and report validation do not measure performance.

## Optional backup snapshots

The current evidence is already in Git. Portable snapshots are an additional backup, kept under
`work/` rather than mixed into the published baseline:

```sh
PYTHONPATH=scripts python3 -m benchmarking.snapshot create --out work/bench-archive/benchmark-data.tar.gz
PYTHONPATH=scripts python3 -m benchmarking.snapshot verify work/bench-archive/benchmark-data.tar.gz
```

The exporter refuses an existing output. It captures the current inputs, canonical records,
embedded samples, environment evidence, Python reproduction tools and generated report.
`snapshot.json` lists each member's SHA-256; an archive checksum is written alongside it.
Verification reproduces the captured report in a private directory without changing the checkout.
This validates evidence consistency and report reproduction, not a new model-checking run.

Keep the exact measured-source bundle when amending history; `git bundle verify` checks it.
The initial source bundle is retained in the maintainer's `work/` archive. Copy backups to an
independent disk or object store for protection against disk loss. A local archive is not an
off-machine backup; the repository does not configure an external backup destination.

## Frozen snapshot provenance

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
These fingerprints identify the measured Rust inputs independently of that amend. Subsequent
fixes do not change the frozen measurements:

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
this document.

## Timeout observations at the freeze point

These are historical observations for the initial snapshot, not a list of current defects.
See [open issues](../docs/issues.md) for remaining validation and performance work.

All used a 600-second bound in the earlier attempts. The extended retries used a single-sample bound
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

## Subsequent validation

The [2026-10-11 fix validation](../docs/research/post-fix-validation.md) records completed CCF and
MCInnerSerial checks, bounded enumeration timeouts, and the single-sample Kafka diagnostic.
It does not change the initial baseline or establish a repeated Kafka speedup.

Documentation-only history cleanup changes Git commit identifiers.
[source-equivalents.toml](environment/source-equivalents.toml) maps the original measured
revisions to reachable revisions with identical Rust/workspace inputs. Measurement files keep
their original identities; this mapping does not relabel or repeat a measurement.
