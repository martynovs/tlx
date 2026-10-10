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

The initial baseline was frozen on 2026-10-11 at 08:51:23 +08:00. The source-equivalent initial commit is found with
`git rev-list --max-parents=0 HEAD`. Measurements retain their original source identity in the
evidence files; history cleanup does not relabel them.
The Rust workspace is unchanged by the documentation and benchmark-tool updates that finalize the
initial commit. [Known issues and baseline provenance](../docs/issues.md) records the source
fingerprints, incomplete configurations and explicitly identified in-progress checkpoints.

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
