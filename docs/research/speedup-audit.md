# Auditing the published speedups above 100x

The frozen store's published table contains ratios far above the typical 5-10x
([performance.md](../performance.md)). This report audits the 13 configurations whose *paired*
tlx/TLC ratio the report publishes above 100x at the 2026-10-11 freeze, states which of them are
supported by evidence, and names the ones that are not what their number suggests. It does not
change the frozen baseline: it adds an inventory tool, a local reproduction method and this record.

## 0. Which rows are candidates

A ratio is published only for a stored tlx row that passes the report's `eligible` selection and
whose recorded TLC baseline matches it under `baselines.historical_match`
([benchmarking.md](../benchmarking.md) section 2 owns the store). Recomputing the candidates with a
different rule (pairing by model name, or taking every record) would treat excluded or incomplete
rows as claims the report never made. `benchmarking.speedup_audit` uses the same two functions:

```sh
PYTHONPATH=scripts python3 -m benchmarking.speedup_audit --threshold 100 --verify-digests
```

The command prints the table below; `--verify-digests` recomputes each candidate's input digest from
the resolved spec, configuration and module closure and compares it with both stored keys. All 13
digests recompute **exactly** (tlx key and TLC key), so the two sides of every ratio were measured
on the same bytes. The 13 candidates reproduce the seed inventory in full; no row was added or lost.

## 1. Cross-cutting findings

**F1 - every denominator is one TLC sample.** The tlx side of all 13 rows has 3/3 complete samples
(`provenance.repetitions`), and the store keeps every raw sample and the discarded warm-up in
`raw_result`. The TLC side is one measurement per key: of the 470 TLC records in the current store,
470 are `aggregation: single-or-legacy-scalar` and only two record any repetition. A ratio built
from one sample has no noise bound: the numerator of the *ratio* is a point estimate on the
collecting instance (F5 shows that instance had contention windows).

**F2 - warm tlx against a cold TLC process.** A measured tlx row is the median of three timed runs
after a discarded warm-up that fills the native cache (benchmarking.md section 1); the cold run is
kept in `raw_result.warmups` but the report prints only the warm time. TLC's number is one process
that includes JVM start, parsing and compilation of the model. The asymmetry is small for the
long-running candidates and large for the short ones:

| candidate | warm tlx | cold tlx | cold / warm |
|---|---:|---:|---:|
| MultiCarElevator/ElevatorLivenessLarge | 0.712 | 3.637 | 5.1x |
| MultiCarElevator/ElevatorLivenessMedium | 0.165 | 1.045 | 6.3x |
| ext/cassandra/Accord_baseline__deep_chain (-compile) | 1.111 | 7.568 | 6.8x |
| bench/ReprRaftMongo | 0.491 | 1.898 | 3.9x |
| PaxosHowToWinATuringAward/MCPaxosSmall | 0.552 | 1.857 | 3.4x |
| ext/mongo/MCRaftMongo (all three rows) | 1.660-14.680 | 1.794-17.940 | 1.08-1.22x |
| bench/RaftMongoT4, byzpaxos/PConProof, MisraReachability/* | 0.096-9.779 | 0.207-11.811 | 1.2-2.8x |

For `ElevatorLivenessLarge` the same data gives 1488.466 / 3.637 = 409x end-to-end including tlx's
own compile, against the published 2090x. The fastest tlx rows (0.1-0.7 s) are the ones where the
omitted cold cost is largest in relative terms, so the section title "Default mode end-to-end vs
TLC" overstates what is compared for exactly the rows this audit is about.

**F3 - two rows compare different searches.** `MCPaxosSmall` (239x) and `PConProof` (181x) are
default-mode runs that applied tlx's automatic symmetry of sets of model values
([usage.md](../usage.md) section 6.3). Their recorded `Optimizations:` line says
`proven; safety checks only`, and the published distinct counts differ from TLC's by 10.8x and
10.6x. Re-running the *same configurations* with `-tlc` (the tier-2 mode) reproduces TLC's counts
exactly, so the reduced search is not a mis-measurement - but the ratio it produces is not an
engine-speed comparison:

| configuration | tlx default (published) | tlx `-tlc` | TLC | like-for-like ratio |
|---|---:|---:|---:|---:|
| PaxosHowToWinATuringAward/MCPaxosSmall | 17153 distinct / 0.58 s | 185369 / 2.7 s | 185369 / 131.9 s | ~49x |
| byzpaxos/PConProof | 95028 / 9.4 s | 1009133 / 21.1-24.5 s | 1009133 / 1769.0 s | ~72-84x |

Both like-for-like ratios are **below the 100x threshold** this audit uses: for these two
configurations the threshold is crossed by the reduction, not by the engine. The reduction itself
checks out. `-stats json` reports `proof: static`, `scope: safety`, `preserved: [V-safe, CEX, D, L]`
for the applied group; the bounded cross-check finds no violation in 1000 states at depth 7 for
each candidate set; and a compatible-mode run with the equivalent declared
`SYMMETRY Perms == Permutations(Acceptor) \cup Permutations(Value)` reproduces the same 17153
distinct states, which is the documented identity ("the state counts ... equal TLC's counts with the
corresponding SYMMETRY added to the cfg"). Their properties are checked: the configured PROPERTY of
`MCPaxosSmall` is `V!Spec` (`Voting!Spec == Init /\ [][Next]_<<votes, maxBal>>`, a safety shape), which
tlx decomposes into a per-step action property exactly as TLC's `processConfigProps` does, and a
falsified copy of that property in the same configuration is reported
(`Error: Action property prop_... is violated.`) in both modes.

The policy that lets a default-mode row pair with an unreduced TLC baseline is written down (the
report header exempts "distinct count outside default mode") and the report prints the distinct
counts next to the ratio, so a reader who knows the rule can see it. What is missing is a label on
the row: nothing in the table or its section text says that a ratio above 100x on this table may be
mostly a reduction. `ElevatorSafetyLarge` (published at 22.72x, below this audit's threshold) is the
same case with `Optimizations: symmetry Permutations(Person)` and 10,365,445 reduced distinct
states against TLC's 59,007,145.

**F4 - the checks that must hold, hold.** For all 13 rows: input digest recomputed equal on both
sides; worker count, instance type, architecture and `cgroup_mib` equal; TLC version equal; verdict
`success` on both sides; `generated`, `distinct` and `depth` equal in 11 of 13 rows (the two
reductions above are the exceptions, and both re-check equal under `-tlc`). Mode contract: the four
`-tlc`/compatible rows (`MCRaftMongo` x2, `bench/ReprRaftMongo`, and the local `-tlc` re-runs of the
reduction cases) satisfy tier 2 exactly, including distinct counts. The local re-runs of every
candidate reproduced the stored counts, verdicts and digests with the release binary built from the
current revision.

**F5 - the property and liveness checks are real, not skipped.** The suspicion this audit was asked
to test - that a large ratio comes from tlx silently dropping liveness - is refuted for these rows:

* The default-mode run of `ElevatorLivenessLarge` prints `Checking 8 branches of temporal
  properties for the complete state space with 405224 total distinct states` and
  `Finished checking temporal properties in 00s`; `ElevatorLivenessMedium` prints 6 branches,
  `bench/RaftMongoT4` and the two `MCRaftMongo` rows 9 branches each, `MCReachable`/`MCParReach` one
  branch each (`Termination`, `Refines`). The branch count is the tableau size, and the run says when
  it checked it.
* The store itself contains three default-mode rows whose verdict is `liveness failure`
  (`FiniteMonotonic/MCDistributedReplicatedLog`, `SpecifyingSystems/RealTime/MCRealTimeHourClock`,
  `bench/ClockSettles`), so the default mode enforces temporal properties.
* Where a temporal property is checked, tlx refuses the automatic symmetry and says so: the `-tlc`
  runs of `MCReachable`/`MCParReach` print
  `Symmetry: ... is provable for safety checking and not declared (not for the temporal properties
  this run checks: under SYMMETRY, TLC can miss or invent their violations).`
* The tlx time is search time, not hidden compile time: for `ElevatorLivenessLarge` the recorded
  coverage census gives `search_thread_s_est` 0.785 s against a 0.712 s wall; the cold compile is
  the discarded 3.637 s warm-up.
* Nothing was terminated early: every candidate's verdict is `success`, `0 states left on queue`,
  and no `count_problems` entry.

**F6 - the collecting instance had contention windows.** [issues.md](../issues.md) lists four
default-mode rows whose sample range over the median exceeds 25%. Their raw samples classify as
follows (`warmups` is the discarded cold run):

| row | samples (wall s) | noise | median vs min | reading |
|---|---|---:|---:|---|
| SpecifyingSystems/AsynchronousInterface/AsynchInterface | 0.055, 0.039, 0.040 | 41% | +2.5% | 40 ms process; one delayed first sample |
| barriers/Barrier | 0.052, 0.041, 0.040 | 30% | +2.5% | same, at recorded load 1.02 |
| Prisoners_Single_Switch/PrisonerSolo | 0.053, 0.042, 0.042 | 26% | 0% | same, at recorded load 1.21 |
| MultiCarElevator/ElevatorSafetyLarge | 99.255, 129.140, 98.571 | 31% | +0.7% | 100 s run; one sample 30% slow, cold run 114 s, recorded load 1.0 |

The three sub-100 ms rows are a measurement-floor effect (a single scheduling delay inflates the
range while the median is unaffected): their published ratios are 9.6-10.3x, not candidates here. The fourth is
different in kind - a long run with one sample 30% slower than the other two, at a recorded load the
collector did not flag. Its published ratio uses the median (99.255 s, within 0.7% of the minimum),
so its own headline is not distorted, but it is direct evidence that a *single* TLC process could
have landed in such a window, and TLC's per-key baseline is exactly one process. With F1 this is the
largest unexplained uncertainty in the biggest numbers.

## 2. Per-case evidence and status

Statuses: **supported** - the ratio is a real end-to-end comparison of the same task, with the
caveats named; **policy-shaped** - the ratio is correct under the documented default-mode rule but is
not an engine-speed comparison; **pending** - the recorded evidence is consistent but one bounded
experiment is needed to name the cause. F1 (one TLC sample per denominator) applies to all 13 rows
and is not repeated in the column.

Every candidate was re-run locally with a release binary using the baseline Rust source (source-equivalent revision `5dc582bde98a677524032753f133dc3692cd2e15`), using the
suite's own argument construction (worker count, tier flag, `-tlc`), on a cold and a warm cache; the
counts, verdicts and content digests below are what the *stored* rows say:

| configuration (mode) | reproduced distinct / generated / depth | reproduced result | local wall s (2 runs) | digest |
|---|---|---|---:|---|
| ElevatorLivenessLarge (default) | 50653 / 230899 / 39 | success | 0.818, 0.761 | verified |
| MCReachable (default, `-tlc`) | 8 / 11 / 6 | success | 0.096, 0.096 | verified |
| RaftMongoT4 (`-compile` 16w) | 1572408 / 21924604 / 29 | success | 5.736 | verified |
| MCRaftMongo (default 1w, `-compile -tlc` 1w, `-compile -tlc` 16w) | 371368 / 5398516 / 21 | success | 10.672, 9.845, 0.885 | verified |
| MCPaxosSmall (default) | 17153 / 127002 / 25 | success | 0.595, 0.580 | verified |
| MCPaxosSmall (`-tlc`) | 185369 / 1361380 / 25 | success | 2.731, 2.674 | verified |
| PConProof (default) | 95028 / 11676837 / 28 | success | 9.437, 9.698 | verified |
| PConProof (`-tlc`) | 1009133 / 115774725 / 28 | success | 24.501, 21.097 | verified |
| Accord_baseline__deep_chain (default, `-compile`) | 179712 / 214191 / 9 | success | 0.823, 0.772 | verified |
| ReprRaftMongo (`-compile -tlc`) | 12003 / 166069 / 16 | success | 0.431 | verified |
| MCParReach (default, `-tlc`) | 393 / 747 / 18 | success | 0.115, 0.111 | verified |
| ElevatorLivenessMedium (default) | 4122 / 14296 / 36 | success | 0.183, 0.186 | verified |

The reproduction used the corpus inputs resolved by `benchmarking.models`/`corpus`, not the digest
recorded in the store, so the digests above are recomputed independently (F4) and not copied.

| # | configuration | ratio | tlx warm / cold s | TLC s | distinct tlx / TLC | status |
|---:|---|---:|---:|---:|---:|---|
| 1 | MultiCarElevator/ElevatorLivenessLarge default 1w | 2090.54x | 0.712 / 3.637 | 1488.466 | 50653 / 50653 | supported (F2) |
| 2 | MisraReachability/MCReachable default 1w | 790.16x | 0.096 / 0.207 | 75.855 | 8 / 8 | pending (TLC phase) |
| 3 | bench/RaftMongoT4 -compile 16w | 310.09x | 6.603 / 8.781 | 2047.532 | 1572408 / 1572408 | supported |
| 4 | ext/mongo/MCRaftMongo -compile 16w -tlc | 270.96x | 1.660 / 1.794 | 449.796 | 371368 / 371368 | supported |
| 5 | ext/mongo/MCRaftMongo default 1w | 242.17x | 14.665 / 16.337 | 3551.468 | 371368 / 371368 | supported (shared denominator with #6) |
| 6 | ext/mongo/MCRaftMongo -compile 1w -tlc | 241.93x | 14.680 / 17.940 | 3551.468 | 371368 / 371368 | supported (duplicate of #5) |
| 7 | PaxosHowToWinATuringAward/MCPaxosSmall default 1w | 238.97x | 0.552 / 1.857 | 131.910 | 17153 / 185369 | policy-shaped (F3) |
| 8 | byzpaxos/PConProof default 1w | 180.89x | 9.779 / 11.811 | 1768.963 | 95028 / 1009133 | policy-shaped (F3) |
| 9 | ext/cassandra/Accord_baseline__deep_chain -compile 1w | 146.60x | 1.111 / 7.568 | 162.878 | 179712 / 179712 | supported (F2) |
| 10 | ext/cassandra/Accord_baseline__deep_chain default 1w | 146.21x | 1.114 / 1.285 | 162.878 | 179712 / 179712 | supported (duplicate of #9) |
| 11 | bench/ReprRaftMongo -compile 1w -tlc | 132.22x | 0.491 / 1.898 | 64.921 | 12003 / 12003 | supported (F2) |
| 12 | MisraReachability/MCParReach default 1w | 111.74x | 0.109 / 0.310 | 12.180 | 393 / 393 | pending (TLC phase) |
| 13 | MultiCarElevator/ElevatorLivenessMedium default 1w | 109.12x | 0.165 / 1.045 | 18.005 | 4122 / 4122 | supported (F2) |

Records: the tlx row and its TLC baseline for each case are the files named in the report's Evidence
column; the inventory JSON emits both paths per row. Every row above also carries the raw samples and
the discarded warm-up in its `raw_result`, so F1/F2 are checkable from the store alone.

### 2.1 The liveness rows (1, 3-6, 13)

The candidate's own evidence says the configured property was checked (F5). The TLC side of the
ratio is dominated by TLC's own liveness machinery, not by search, and the store shows it without a
new experiment:

* `ElevatorLivenessLarge`: TLC took 1488.466 s for 50,653 distinct states (29 ms/state, 6.9 GiB RSS)
  while the sibling safety configuration `ElevatorSafetyLarge` took 2254.915 s for 59,007,145
  distinct states (38 us/state, 6.1 GiB) on the same instance type - a 770x difference in state rate
  and 1300x in bytes per state on the same spec.
* `ElevatorLivenessMedium`: 18.005 s for 4,122 states (4.4 ms/state), again against the same
  safety sibling's 38 us/state.
* `bench/RaftMongoT4` and `ext/mongo/MCRaftMongo`: nine leads-to properties under strong fairness;
  TLC's 16-worker baseline spends 26,201 CPU-seconds (12.8 cores busy) on 1.57M distinct states,
  16.7 ms of CPU per distinct state against 38 us per state on the safety sibling above.

Caveat: the ratio mixes two different liveness designs (TLC's in-search tableau against tlx's
offline check after the search) with two different parallel strategies, so it is not a per-state
engine ratio. Its direction is not in doubt; its size across machines is. The manifest's own
`runtime` for `ElevatorLivenessLarge` is 11 minutes, against 24.8 minutes on the collecting
instance, so the same comparison on the author's machine would read ~930x.

### 2.2 The MisraReachability rows (2, 12)

`MCReachable` is 8 distinct states and 11 edges; `MCParReach` is 393 and 747. Their TLC times
(75.855 s and 12.180 s) are 9.5 s and 31 ms *per distinct state*. Both configurations are ordinary
safety-sized models whose cost in TLC is serial, which the store shows directly:

* The same key at 16 workers is **slower** than at one worker: 102.672 s against 75.855 s, with
  `cpu_s / wall_s` = 1.19 (about one core busy), where `MCBoulanger` (243.75 s -> 29.588 s) and
  `MCRaftMongo` (3551.468 s -> 449.796 s) use 12.6-14.0 cores at the same worker count. TLC gains
  nothing from 16 workers here, so its time is not search work.
* The tlx side of the comparison is search: the census recorded with the row reports 84 evaluations
  and 0.0145 s of estimated search-thread time, and the whole run is a property-checked search over
  the same 8 states (its cold warm-up, which cannot hide a compile, is 0.207 s).
* The configurations are the unusual part: `Succ <- ConnectedToSomeButNotAll` substitutes a
  `CHOOSE succ \in [Nodes -> SUBSET Nodes] : ...` over a 65536-function space, and
  `Seq <- LimitedSeq` bounds `Seq(Nodes)` for `ExistsPath`; four of `MCReachable`'s invariants
  (`Inv2`, `Inv3`, `PartialCorrectness` through `Reachable = ReachableFrom({Root})`) consume them on
  every evaluation.

What is *not* established is which TLC phase those seconds are in: the store keeps normalized
baselines, not TLC's stdout, and the collecting history file
`bench/history/tlc-1813307-SA5.4XLARGE32.json` named by their provenance is not in the published
tree (the raw runner histories live under the maintainer's `work/` archive). The pending experiment
is one TLC JVM with its stdout kept, which prints progress timestamps and the final
`Finished in` time, so BFS time separates from post-search liveness time; an optional second run of
a reduced configuration (only `TypeOK`) would confirm whether the per-state invariant evaluation is
the cost. Either way the *ratio* is real (TLC really takes ~76 s on this configuration), but until
that run exists this audit does not claim to know what TLC is doing with them.

```sh
PYTHONPATH=scripts python3 scripts/tlc.py third_party/Examples/specifications/MisraReachability/MCReachable.tla -config third_party/Examples/specifications/MisraReachability/MCReachable.cfg -workers 1 --allow-local-heavy
```

That configuration's TLC time exceeds the local refusal threshold, so it needs an approved TLC slot
(the two local JVM slots are shared across agents) and `--allow-local-heavy`; the reduced variant is
the same spec with a scratch configuration under `target/` that keeps only `TypeOK` and drops the
`PROPERTY`, which changes the key and never the shipped inputs.

## 3. Proposed bounded repeat protocol

To replace the single-sample denominators and the warm/cold asymmetry with a defensible number, for
this 13-row set only (bounded by construction: the whole set costs TLC ~2.7 hours of CPU-serial time
at the recorded times, and most of it is dominated by four rows):

1. One isolated instance per run, no other measurement on it, `load` recorded before and after each
   sample (F6 shows the recorded load did not flag a 30% excursion, so it is evidence, not proof).
2. Three TLC samples per key, interleaved with three tlx samples of the same configuration, and
   report the median *and* the range/median; a ratio is quoted only with its TLC range. `bench.py ab`
   already interleaves A/B; the TLC side needs the same treatment instead of one cached process per
   key.
3. For every default-mode row, a companion `-tlc` run of the same configuration, so each published
   ratio has a like-for-like twin and any `Optimizations:` line has a number attached to it (F3).
4. Record the cold tlx time (empty `TLX_CACHE`) in the table, not only in the raw result (F2), or
   rename the section so it says warm-cache.
5. For the liveness rows, keep the branch count and the property names next to the time (F5), and
   record TLC's phase split for at least the two `MisraReachability` configurations.

## 4. What this audit does not claim

* It does not claim any of the 13 rows has a wrong verdict, a wrong state count under its mode
  contract, or a wrong input digest: those all check out, including the two reduced searches, which
  match TLC exactly under `-tlc`.
* It does not claim the reductions are unsound: their proof is static, documented, cross-checked in
  bounds and reproducible as declared `SYMMETRY`.
* It does not claim the large ratios are fabricated. `ElevatorLivenessLarge`, `RaftMongoT4`,
  `MCRaftMongo` and `Accord` are genuine end-to-end differences on the same inputs and counts.
* It does claim that the six rows with a cold/warm gap above ~3x, the two reduction rows, and the
  two rows whose TLC cost is not search time (2, 12) are not yet evidence for the *size* of the
  number they publish, and that the missing TLC repeats (F1) and the unverified TLC phase of the
  `MisraReachability` rows are the two items to fix first.
