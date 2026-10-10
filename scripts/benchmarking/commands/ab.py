"""`bench.py ab`: an interleaved A/B of tlx binaries on the same models.

`benchmarking/cli.py` owns the parser and `scripts/bench.py` dispatches; the
warm-up, sampling and summary of the pass live here (CODE-03). The shared
helpers (`bench.tlx`, `bench.load_records`) and the summary handler a test
replaces (`bench._ab_summary`) are reached through the entry point module, the
way `bench_profile`/`bench_ttfr` do: it is the namespace a test patches a pass
in through, and the entry point publishes itself under its import name, so that
lookup is the running module and not a second copy of it (HARNESS-03).
"""
import json, math, os, statistics, subprocess, sys, time

import tempdirs
from benchmarking import models as bench_models, machine as bench_machine
from benchmarking.records import COMPLETE, normal_exits
from benchmarking import bench_budget
from tlxlib import ROOT


class Hogs:
    """`n` processes spinning on a core each while in use (an oversubscribed
    machine)."""

    def __init__(self, n):
        self.n, self.ps = n, []

    def __enter__(self):
        spin = "while True:\n    x = sum(range(10000))\n"
        self.ps = [subprocess.Popen([sys.executable, "-c", spin]) for _ in range(self.n)]
        return self

    def __exit__(self, *exc):
        for p in self.ps:
            p.kill()
        for p in self.ps:
            p.wait()


def run_stats(path):
    """The metrics `ab --stats` records from a `-stats json` file."""
    try:
        with open(path) as stream:
            s = json.load(stream)
    except (OSError, ValueError):
        return {}
    ph, mem, sec = s.get("phases", {}), s.get("memory", {}), s.get("sections", {})
    regions, by_var = 0, {}
    for title, table in sec.items():
        if not title.endswith("mixed-mode region evaluations by state variable read"):
            continue
        for line in table.splitlines():
            parts = line.split()
            if parts[:2] == ["all", "regions"]:
                regions += int(parts[2])
            elif len(parts) > 2 and parts[0] == "reading" and parts[2].isdigit():
                by_var[parts[1]] = by_var.get(parts[1], 0) + int(parts[2])
    # Value store slots: "  NAME   Ref (1 word)" in the layout slot report.
    ref_vars = [p[0] for p in (line.split() for line in sec.get("layout slots (final layout: shape, words; how it was chosen)", "").splitlines())
                if len(p) > 1 and p[1] == "Ref"]
    return {"raw_stats": s, "search_s": ph.get("search"), "region_evals": regions, "regions_by_var": by_var, "ref_vars": ref_vars,
            "ref_ops": (ph.get("value store operations (closure IR)") or 0) + (ph.get("value store operations (native ref_op)") or 0),
            "store_bytes": (mem.get("value store nodes and payloads") or 0) + (mem.get("value store index (est.)") or 0),
            "dyn_bytes": (mem.get("Dyn interner tables") or 0) + (mem.get("Dyn interner value heap (retained)") or 0),
            "repeated": ph.get("states expanded again (layout restarts, resumes)") or 0, "stats_rss": mem.get("peak RSS")}


# (c-m05: an A/B that changes one execution backend must declare it, so the
# pass cannot silently time another backend and warm the wrong cache.)
_AB_TRUTHY = {'1', 'true', 'yes', 'on'}


def ab_backend(env):
    """The execution backend an environment selects: `c` (`TLX_JIT=0`), `jit`
    (`TLX_JIT=1`/`only`), `closure` (`TLX_NO_NATIVE`) or `default` (the tiered
    default, which is no single backend)."""
    if str(env.get('TLX_NO_NATIVE', '')).strip().lower() in _AB_TRUTHY:
        return 'closure'
    jit = str(env.get('TLX_JIT', '')).strip().lower()
    if jit in ('0', 'off', 'false', 'no'):
        return 'c'
    if jit in ('1', 'only'):
        return 'jit'
    return 'default'


def ab_backend_env(backend):
    """The `NAME:K=V` value `--env` needs to select `backend` ('' for the
    default tiering, which needs no environment variable)."""
    return {'c': 'TLX_JIT=0', 'jit': 'TLX_JIT=1', 'closure': 'TLX_NO_NATIVE=1'}.get(backend, '')


class AbWarmupFailure(Exception):
    """A warm-up ended in a non-timeout unknown verdict or a crash (bpa-abi):
    the pass stops instead of comparing a broken binary's missing samples."""


def measured_costs(keys, workers=1, mode='-compile', root=None):
    """The measured cost of each named model in `mode`, as `{model: seconds}`,
    for a deadline admission that must reserve what a run really costs on this
    machine rather than another tool's estimate (c-r09, c-d09). Only the named
    models, in the mode this pass measures and at its worker count, are looked
    up; missing or unreadable history is not an error (the caller then keeps
    the shared admission's existing fallback)."""
    import bench
    root = root or ROOT
    wanted = {k for k in keys if k}
    if not wanted:
        return {}
    try:
        records = bench.load_records(root)
    except (OSError, ValueError):
        return {}
    walls = {}
    for r in records:
        if r.get('tool') == 'tlc' or r.get('model') not in wanted:
            continue
        if r.get('mode') != mode or r.get('workers') != workers:
            continue
        if r['counts'].get('result') not in COMPLETE:
            continue
        wall = r['metrics'].get('wall_s')
        if isinstance(wall, (int, float)) and wall > 0:
            walls.setdefault(r['model'], []).append(wall)
    return {k: statistics.median(v) for k, v in walls.items()}


def cmd_ab(a):
    import bench
    bins = []
    for b in a.bin:
        name, _, path = b.partition("=")
        if not name or not path:
            sys.exit(f"--bin expects NAME=PATH, got {b!r}")
        path = os.path.abspath(path)
        if not os.access(path, os.X_OK):
            sys.exit(f"not an executable: {path}")
        bins.append((name, path))
    if len(bins) < 2:
        sys.exit("ab needs at least two --bin")
    envs = {n: {} for n, _ in bins}
    for e in a.env:
        name, _, kv = e.partition(":")
        k, eq, v = kv.partition("=")
        if name not in envs or not k or not eq:
            sys.exit(f"--env expects NAME:K=V with NAME a --bin, got {e!r}")
        envs[name][k] = v
    # (More tlx arguments of one binary, e.g. -tlc against the default mode
    # of the same binary.)
    extra = {n: [] for n, _ in bins}
    for e in getattr(a, "bin_args", []):
        name, _, words = e.partition(":")
        if name not in extra:
            sys.exit(f"--bin-args expects NAME:ARGS with NAME a --bin, got {e!r}")
        extra[name] += words.split()
    # (HARNESS-19: the revision of an external binary is adopted only when the
    # command states it; a label such as "new" or "after" proves nothing, so a
    # binary without --bin-rev is recorded as unknown.)
    bin_revs = {}
    for spec in getattr(a, "bin_rev", []):
        name, _, rev = spec.partition("=")
        if name not in {n for n, _ in bins} or not rev:
            sys.exit(f"--bin-rev expects NAME=REV with NAME a --bin, got {spec!r}")
        bin_revs[name] = rev
    # (c-m05: an A/B of one execution backend must declare it, and every run
    # must select it, so a pass cannot silently time a different backend and
    # warm the wrong cache.)
    backend = getattr(a, "backend", "")
    if backend and backend != "default":
        wrong = [n for n, _ in bins if ab_backend(envs[n]) != backend]
        if wrong:
            sys.exit(f"--backend {backend} but {', '.join(wrong)} does not select it; "
                     f"pass --env {wrong[0]}:{ab_backend_env(backend)}")
    d = bench_models.oracle([k.strip() for k in a.models.split(",")])
    keys = [k.strip() for k in a.models.split(",") if k.strip()]
    for k in keys:
        if k not in d:
            # (A manifest model without an oracle result, or bench/<name>.)
            d[k] = bench_models.scale_models([k])[k]
    args = a.args.split()
    workers = int(args[args.index("-workers") + 1]) if "-workers" in args else 1
    # (c-r09: admit a model by what it has already cost on this machine - the
    # median of its recorded complete runs in the mode this pass measures - not
    # by a different tool's estimate, which is what let a pass skip a model it
    # could have finished. Without history the estimate stays unset and the
    # shared admission keeps its existing fallback.)
    costs = measured_costs(keys, workers, "-interp" if "-interp" in args else "-compile")
    for k in keys:
        if k in costs:
            d[k] = {**d[k], "estimate_s": costs[k]}
    bin_args = {n: (bench.interp_args(p) if a.interp else []) + args + extra[n] for n, p in bins}
    env_desc = "".join(f"; {n} with {' '.join(f'{k}={v}' for k, v in e.items())}" for n, e in envs.items() if e)
    env_desc += "".join(f"; {n} also {' '.join(x)}" for n, x in extra.items() if x)
    print(f"ab on {bench_machine.machine_desc()}: {', '.join(f'{n}={p}' for n, p in bins)}; args {' '.join(args)}{' (interpreter)' if a.interp else ''}"
          f"{env_desc}; {a.repeat} rounds{f'; {a.hog} CPU hogs' if a.hog else ''}", flush=True)
    res = {(k, n): [] for k in keys for n, _ in bins}
    samples = []
    with tempdirs.TemporaryDirectory(prefix="tlx-ab-") as stats_dir:
        try:
            _ab_samples(a, bins, keys, d, bin_args, envs, res, os.path.join(stats_dir, "stats.json"),
                        checkpoints=samples)
        except AbWarmupFailure as error:
            sys.exit(f"ab aborted: {error}")
    summary = bench._ab_summary(a, bins, keys, d, args, envs, res, bin_revs=bin_revs, checkpoints=samples,
                                backend=backend)
    # (c-r05, c-r09: a named model without a complete sample set - a skipped or
    # crashed round - is not a green pass; the non-zero exit is what a caller
    # sees, and the printed line names the models that missed it.)
    incomplete = sorted({k for k in keys if any(not summary[f"{k}#{n}"]["complete"] for n, _ in bins)})
    if incomplete:
        print(f"ab incomplete: {', '.join(incomplete)} did not complete every round", flush=True)
        return 1
    return 0


def _ab_warmup_failure(k, n, r):
    """The reason a warm-up cannot start a measured pass, or None (bpa-abi).

    A timeout or a budget refusal is a censored sample, not a broken binary. An
    unknown verdict or a crash - a missing state count, or a child exit that
    does not match the verdict - would make the pass compare missing samples
    with a plausible-looking banner, so it stops instead and keeps the raw
    evidence in the partial artifact."""
    import bench
    if r.get("result") in ("timeout", "skipped"):
        return None
    normal = normal_exits().get(r.get("result"))
    if normal is not None and (r.get("exit") is None or r.get("exit") in normal):
        return None  # a normal verdict; a missing exit is not evidence of a crash
    detail = (r.get("raw") or {}).get("output") or ""
    tail = next((line.strip() for line in reversed(detail.splitlines()) if line.strip()), "")
    return (f"warm-up {k} {n} ended {r.get('result')} (exit {r.get('exit')})"
            + (f": {tail[:200]}" if tail else ""))


def _ab_samples(a, bins, keys, d, bin_args, envs, res, stats_file, *, checkpoints=None):
    """The interleaved runs of `cmd_ab` into `res`; `bin_args`: each
    binary's tlx arguments.

    Every warm-up and sample is written as it completes - to `checkpoints` (the
    per-sample shape the suite uses: model, variant, round, warmup and the run's
    fields, phases included) and, with the samples so far, to
    `--out`.partial.json - so an interrupted or aborted pass keeps its evidence
    (c-d15, bpa-abi)."""
    import bench
    if a.stats:
        bin_args = {n: x + ["-stats", f"json={stats_file}"] for n, x in bin_args.items()}

    def partial():
        if not getattr(a, "out", ""):
            return
        path = os.path.abspath(a.out + ".partial.json")
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path + ".tmp", "w") as stream:
            json.dump({"partial": True, "bins": dict(bins), "args": bin_args, "env": envs,
                       "samples": list(checkpoints or []),
                       "runs": {f"{model}#{name}": rows for (model, name), rows in res.items()}}, stream)
        os.replace(path + ".tmp", path)

    def run(k, n, p):
        # (c-r08: a refused or crashed run must not inherit the previous
        # sample's -stats document, and only a real verdict keeps stats.)
        if a.stats and os.path.exists(stats_file):
            os.unlink(stats_file)
        return bench.tlx(d[k], bin_args[n] + bench_models.bench_flags(d[k]), a.timeout, exe=p, env=envs[n], raw=True)

    def sample_row(r):
        stats = run_stats(stats_file) if a.stats and r.get("result") in normal_exits() else {}
        row = {"violated": r.get("violated"), "trace_length": r.get("trace_length"), "wall": r["wall_s"], "cpu": r["cpu_s"],
               "rss": r["maxrss_mib"], "distinct": r.get("distinct"), "compatibility": r.get("compatibility"),
               "result": r.get("result"), "exit": r.get("exit"), "generated": r.get("generated"), "depth": r.get("depth"),
               "load": round(os.getloadavg()[0], 2), **stats}
        # (bpa-abi: keep the raw stream and exit of a run whose parsed verdict
        # does not prove it succeeded, so a crash cannot read as a missing
        # performance sample.)
        if r.get("result") not in normal_exits() and r.get("raw"):
            row["raw"] = r["raw"]
        return row

    def note(k, n, round_number, warmup, r):
        if checkpoints is not None:
            checkpoints.append({"model": k, "variant": n, "round": round_number, "warmup": warmup, **r})

    with Hogs(a.hog):
        for k in keys if not a.no_warmup else []:
            for n, p in bins:
                r = run(k, n, p)
                note(k, n, 0, True, r)
                partial()
                print(f"  warm-up {k} {n}: {r.get('result')}, {r.get('distinct')} states, cpu {r['cpu_s']:.2f}s", flush=True)
                failure = _ab_warmup_failure(k, n, r)
                if failure:
                    raise AbWarmupFailure(failure)
        for i in range(a.repeat):
            order = bins[i % len(bins):] + bins[:i % len(bins)]
            for k in keys:
                for n, p in order:
                    r = run(k, n, p)
                    res[k, n].append(sample_row(r))
                    note(k, n, i + 1, False, r)
                    partial()
            print(f"  round {i + 1}/{a.repeat} done", flush=True)


# Normal tlx exits, owned by crates/tlx-cli/src/report.rs (EXIT_*).


def _ab_summary(a, bins, keys, d, args, envs, res, *, bin_revs=None, checkpoints=None, backend=""):
    import bench
    base = bins[0][0]
    print(f"\n{'model':<48} {'binary':<14} {'cpu min':>8} {'cpu med':>8} {'wall min':>8} {'wall med':>8} {'rss med':>8} "
          f"{'cpu med/' + base:>14} {'wall med/' + base:>15}  distinct")
    summary = {}

    def completed(rows):
        # A complete-looking banner cannot certify the producer's exit.
        return [x for x in rows if x.get("distinct") is not None and
                x.get("exit") in normal_exits().get(x.get("result"), ())]

    for k in keys:
        bx = completed(res[k, base])
        b_cpu = statistics.median(x["cpu"] for x in bx) if bx else 0
        b_wall = statistics.median(x["wall"] for x in bx) if bx else 0
        for n, _ in bins:
            xs = completed(res[k, n])
            full = len(xs) == len(bx) == a.repeat
            cpu, wall = [x["cpu"] for x in xs] or [0], [x["wall"] for x in xs] or [0]
            s = {"cpu_min": min(cpu), "cpu_median": statistics.median(cpu), "wall_min": min(wall), "wall_median": statistics.median(wall),
                 "rss_median": statistics.median(x["rss"] for x in xs) if xs else 0,
                 "completed_samples": len(xs), "complete": len(xs) == a.repeat,
                 "cpu_ratio_median": statistics.median(cpu) / b_cpu if full and b_cpu else None,
                 "wall_ratio_median": statistics.median(wall) / b_wall if full and b_wall else None,
                 "distinct": sorted({x["distinct"] for x in res[k, n]}, key=str),
                 "results": sorted({str(x["result"]) for x in res[k, n]})}
            if a.stats:
                med = lambda f: statistics.median([x.get(f) or 0 for x in xs] or [0])
                bmed = lambda f: statistics.median([x.get(f) or 0 for x in bx] or [0])
                for f in ("search_s", "region_evals", "ref_ops", "store_bytes", "dyn_bytes", "repeated"):
                    s[f + "_median"] = med(f)
                s["search_ratio_median"] = med("search_s") / bmed("search_s") if full and bmed("search_s") else None
                s["region_ratio_median"] = med("region_evals") / bmed("region_evals") if full and bmed("region_evals") else None
                # Region calls reading the value store slots of the second
                # binary (the store's own variables, e.g. message bags).
                store_vars = sorted({v for x in res[k, bins[1][0]] for v in x.get("ref_vars") or []})
                if 0 < len(store_vars) <= 3:
                    s["store_vars"] = store_vars
                    for f, xs_ in (("store_var_regions_median", xs), ("base_store_var_regions", bx)):
                        s[f] = statistics.median([sum((x.get("regions_by_var") or {}).get(v, 0) for v in store_vars) for x in xs_] or [0])
            summary[f"{k}#{n}"] = s
            cpu_ratio = f"{s['cpu_ratio_median']:.3f}" if s['cpu_ratio_median'] is not None else "-"
            wall_ratio = f"{s['wall_ratio_median']:.3f}" if s['wall_ratio_median'] is not None else "-"
            print(f"{k:<48} {n:<14} {s['cpu_min']:8.2f} {s['cpu_median']:8.2f} {s['wall_min']:8.2f} {s['wall_median']:8.2f} "
                  f"{s['rss_median']:8.1f} {cpu_ratio:>14} {wall_ratio:>15}  {','.join(map(str, s['distinct']))} ({len(xs)}/{a.repeat} complete)")
            if a.stats:
                print(f"{'':<48} {'':<14} search med {s['search_s_median']:.3f}s ({s['search_ratio_median'] or 0:.3f}), regions {s['region_evals_median']:.0f} "
                      f"({s['region_ratio_median'] or 0:.3f}), store ops {s['ref_ops_median']:.0f}, store {s['store_bytes_median'] / 2**20:.1f} MiB, "
                      f"Dyn {s['dyn_bytes_median'] / 2**20:.1f} MiB, repeated {s['repeated_median']:.0f}")
                if "store_vars" in s:
                    b = s["base_store_var_regions"]
                    print(f"{'':<48} {'':<14} regions reading {','.join(s['store_vars'])} {s['store_var_regions_median']:.0f} "
                          f"({s['store_var_regions_median'] / b if b else 0:.3f})")
    tlc_side = {}
    if not a.tlc_baseline:
        from benchmarking.baselines import find_baseline, raw_baseline
        from benchmarking.records import instance_info
        w = int(args[args.index("-workers") + 1]) if "-workers" in args else 1
        version = bench_machine.tla2tools_info().get("revision") or bench_machine.tla2tools_info().get("short")
        for k in keys:
            baseline = find_baseline(d[k], w, version=version, instance=instance_info(bench_machine.machine_desc())["type"])
            if baseline:
                tlc_side[k] = {**raw_baseline(baseline), "workers": w}
            else:
                print(f"TLC baseline missing/stale: {k} {w}w; use bench.py tlcbase --missing")
        if tlc_side:
            print(f"\nTLC {version} baselines of the keyed store ({w} workers, this machine type): speedup = TLC time / median time of the binary")
            print(f"{'model':<48} {'TLC wall':>9} {'TLC cpu':>9}  " + "  ".join(f"{n + ' wall':>14} {n + ' cpu':>14}" for n, _ in bins))
            eligible = {k: r for k, r in tlc_side.items() if all(
                summary[f"{k}#{n}"]["complete"] and summary[f"{k}#{n}"]["wall_median"] > 0 and
                summary[f"{k}#{n}"]["cpu_median"] > 0 for n, _ in bins)}
            for k, r in eligible.items():
                cells = "  ".join(f"{r['wall_s'] / summary[f'{k}#{n}']['wall_median']:13.2f}× {r['cpu_s'] / summary[f'{k}#{n}']['cpu_median']:13.2f}×"
                                  for n, _ in bins)
                print(f"{k:<48} {r['wall_s']:9.2f} {r['cpu_s']:9.2f}  {cells}")
            for n, _ in bins if eligible else []:
                g = math.exp(statistics.fmean(math.log(r["wall_s"] / summary[f"{k}#{n}"]["wall_median"]) for k, r in eligible.items()))
                print(f"geometric mean wall speedup of {n} over TLC: {g:.2f}× ({len(eligible)} of {len(keys)} models)")
    if a.tlc_baseline:
        bh, base_runs = bench.load_tlc_baseline(a.tlc_baseline)
        w = int(args[args.index("-workers") + 1]) if "-workers" in args[:-1] else 1
        print(f"\nTLC baseline {os.path.basename(bench.history_path(a.tlc_baseline))} ({w} workers; TLC {bh.get('tla2tools', {}).get('short')} "
              f"on {bh.get('machine')}): speedup = TLC time / median time of the binary")
        print(f"{'model':<48} {'TLC wall':>9} {'TLC cpu':>9}  " + "  ".join(f"{n + ' wall':>14} {n + ' cpu':>14}" for n, _ in bins))
        for k in keys:
            r = base_runs.get((k, w))
            if not r or not bench.ok_result(r) or any(not summary[f'{k}#{n}']['complete'] or
                                                      summary[f'{k}#{n}']['wall_median'] <= 0 or
                                                      summary[f'{k}#{n}']['cpu_median'] <= 0 for n, _ in bins):
                print(f"{k:<48} {'-' if not r else r.get('result'):>9}")
                continue
            tlc_side[k] = {**r, "workers": w}
            cells = "  ".join(f"{r['wall_s'] / summary[f'{k}#{n}']['wall_median']:13.2f}× {r['cpu_s'] / summary[f'{k}#{n}']['cpu_median']:13.2f}×"
                              for n, _ in bins)
            print(f"{k:<48} {r['wall_s']:9.2f} {r['cpu_s']:9.2f}  {cells}")
    if a.out:
        os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
        header = {"commit": bench_machine.git_commit(), "date": time.strftime("%Y-%m-%d %H:%M:%S"), "machine": bench_machine.machine_desc(), "args": args,
                  "diagnostic": a.stats, "budget": {"skipped": bench_budget.current().skipped}, "compile_failures": bench_budget.current().failures,
                  "bins": dict(bins), "env": envs, "hogs": a.hog,
                  # (HARNESS-19: the artifact states the external binaries' revision
                  # or leaves it null; a label never supplies it. c-m05: the declared
                  # backend is recorded, so the pass's provenance says what it timed.)
                  "bin_revs": dict(bin_revs or {}), "backend": backend,
                  "summary": summary, "runs": {f"{k}#{n}": v for (k, n), v in res.items()},
                  **({"tlc_baseline": {"file": os.path.basename(bench.history_path(a.tlc_baseline)) if a.tlc_baseline else "canonical-store", "runs": tlc_side}} if tlc_side else {})}
        if checkpoints is not None:
            header["samples"] = list(checkpoints)
        with open(a.out, "w") as stream:
            json.dump(header, stream, indent=1, sort_keys=True)
        from benchmarking.legacy import ingest
        ingest(a.out)
        print(f"recorded in {a.out}")
    # The caller checks completeness from this mapping; returning it keeps the
    # verdict and the printed/written summary one source (c-r05).
    return summary
