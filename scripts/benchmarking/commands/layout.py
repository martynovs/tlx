"""`bench.py layout`: how the compiled state layout spends a run's time.

`benchmarking/cli.py` owns the parser and `scripts/bench.py` dispatches; the
per-binary layout pass lives here (CODE-03), reaching the shared helpers
through the entry point module (`bench.tlx`, `bench.read_json`) -- the
namespace a test patches a pass in through (HARNESS-03: the entry point
publishes itself under its import name, so that lookup is the running module,
not a second copy of it).
"""
import json, os, sys, tempfile, time

from benchmarking import models as bench_models, machine as bench_machine


LAYOUT_KEYS = ["pre-run", "pre-run states", "restarts (layout miss)", "resumed searches", "restarts from scratch",
               "fallbacks to dynamic values", "model build (lowering, C generation)", "search", "total"]


def cmd_layout(a):
    import bench
    # The pass measures binaries: it reports the layout of each `--bin` and
    # compares every later one with the first, so a run without one has no
    # result and no baseline. Refused before the header, the model resolution
    # and any run, like `ab needs at least two --bin` beside it, rather than
    # indexing an empty list after doing the work (and printing it).
    if not a.bin:
        sys.exit("layout needs at least one --bin NAME=PATH")
    bins = []
    for b in a.bin:
        name, _, path = b.partition("=")
        if not name or not path or not os.access(os.path.abspath(path), os.X_OK):
            sys.exit(f"--bin expects NAME=PATH of an executable, got {b!r}")
        bins.append((name, os.path.abspath(path)))
    keys = [k.strip() for k in a.models.split(",") if k.strip()]
    d = bench_models.oracle(keys)
    if a.models:
        for k in keys:
            if k not in d:
                sys.exit(f"unknown model {k}")
    else:
        keys = sorted(k for k, m in d.items() if m["tlc"].get("result") is not None and a.filter in k)
    args = a.args.split()
    print(f"layout on {bench_machine.machine_desc()}: {', '.join(f'{n}={p}' for n, p in bins)}; args {' '.join(args)}; {len(keys)} models", flush=True)

    def one(job):
        k, (n, exe) = job
        fd, path = tempfile.mkstemp(prefix="tlx-layout-", suffix=".json")
        os.close(fd)
        try:
            r = bench.tlx(d[k], args + bench_models.bench_flags(d[k]) + ["-stats", f"json={path}"], a.timeout, exe=exe)
            try:
                ph = (bench.read_json(path) or {}).get("phases", {})
            except (OSError, ValueError):
                ph = {}
        finally:
            os.unlink(path)
        rec = {"result": r.get("result"), "distinct": r.get("distinct"), "wall": r.get("wall_s")}
        rec.update({x: ph.get(x) for x in LAYOUT_KEYS})
        return k, n, rec

    from concurrent.futures import ThreadPoolExecutor
    res = {}
    with ThreadPoolExecutor(max(1, a.jobs)) as ex:
        for k, n, rec in ex.map(one, [(k, b) for k in keys for b in bins]):
            res.setdefault(k, {})[n] = rec
            print(f"  {k:<60} {n:<10} {str(rec['result']):<8} pre-run {rec['pre-run'] or 0:6.3f}s {rec['pre-run states'] or 0:>6} states, "
                  f"misses {rec['restarts (layout miss)'] or 0}, total {rec['total'] or 0:7.3f}s", flush=True)
    count = lambda rec, x: rec.get(x) or 0
    base = bins[0][0]
    print(f"\n{'binary':<10} {'models':>6} {'pre-run s':>10} {'pre-run states':>15} {'misses':>7} {'resumed':>8} {'from scratch':>13} {'fallbacks':>10} {'total s':>9}")
    for n, _ in bins:
        rs = [res[k][n] for k in keys]
        print(f"{n:<10} {len(rs):>6} {sum(count(r, 'pre-run') for r in rs):10.2f} {sum(count(r, 'pre-run states') for r in rs):>15} "
              f"{sum(count(r, 'restarts (layout miss)') for r in rs):>7} {sum(count(r, 'resumed searches') for r in rs):>8} "
              f"{sum(count(r, 'restarts from scratch') for r in rs):>13} {sum(count(r, 'fallbacks to dynamic values') for r in rs):>10} "
              f"{sum(count(r, 'total') for r in rs):9.2f}")
    for n, _ in bins[1:]:
        diff = [k for k in keys if any(count(res[k][n], x) != count(res[k][base], x) for x in ("restarts (layout miss)", "restarts from scratch", "fallbacks to dynamic values"))
                or ("-tlc" in args and res[k][n]["distinct"] != res[k][base]["distinct"]) or res[k][n]["result"] != res[k][base]["result"]]
        print(f"\n{n} vs {base}: {len(diff)} models with other restarts, fallbacks, results or distinct states")
        for k in diff:
            x, y = res[k][base], res[k][n]
            print(f"  {k:<60} misses {count(x, 'restarts (layout miss)')} -> {count(y, 'restarts (layout miss)')}, "
                  f"from scratch {count(x, 'restarts from scratch')} -> {count(y, 'restarts from scratch')}, "
                  f"fallbacks {count(x, 'fallbacks to dynamic values')} -> {count(y, 'fallbacks to dynamic values')}, "
                  f"{x['result']}/{x['distinct']} -> {y['result']}/{y['distinct']}")
    if a.out:
        os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
        with open(a.out, "w") as stream:
            json.dump({"commit": bench_machine.git_commit(), "date": time.strftime("%Y-%m-%d %H:%M:%S"), "machine": bench_machine.machine_desc(), "args": args,
                       "bins": dict(bins), "runs": res}, stream, indent=1, sort_keys=True)
        print(f"recorded in {a.out}")
