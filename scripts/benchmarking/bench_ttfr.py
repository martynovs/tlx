#!/usr/bin/env python3
"""Time to first result (bench.py ttfr, ttfr-report): end-to-end wall time
of `tlx check` with its default flags against TLC with its default
`-workers 1`, as a user runs them, for every corpus model with a TLC oracle
(and the external models of --external tiers).

Per model and binary (--bin NAME=PATH, repeatable; no tier flag, only
-workers 1 and the model's simulation flags):
  cold   one run with an empty TLX_CACHE (a user's first run of the model);
         a C build it leaves running is waited for outside the measured time
  warm   --warm more runs with the cache the cold run filled (min)
TLC: read from the canonical keyed store; collected once only when the current
model/config digest, TLC revision, workers and instance type have no valid key.
Legacy baseline options remain accepted as scheduling hints, not cache authority.
A tlx run is cut off at max(--cap-factor x TLC, --cap-min) seconds.
Models where the --stats-bin binary is slower than --near x TLC (cold or
warm) get one more cold run with `-stats json` for the phase breakdown
(stats cost time, so never in the measured runs).

The output (--out, JSON) is rewritten after every model, so a run cut short
keeps what it measured; --resume skips the models already in it. Models run
in increasing TLC time, in --lanes parallel lanes (1: one at a time).
`ttfr-report FILE` imports measurements and regenerates docs/performance.md.

The exit status is an explicit outcome over the models this run scheduled
(never inferred from the last sample); the file is persisted first, so a
non-zero status still leaves the partial measurements:

  0  every scheduled model completed and agreed with TLC
  1  at least one model raised an exception or failed to compile
  2  at least one model has no usable measurement: it was skipped, every run
     was cut off at the cap or unparsed, or it was never measured
  3  every scheduled model was measured but at least one disagrees with TLC
     (result kind, or distinct states under -tlc)

When several apply, error > incomplete > mismatch; status 0 requires a
nonempty selection and no error, incomplete or mismatched model.
"""

from benchmarking import models as bench_models, machine as bench_machine
import tempdirs

import json, math, os, shutil, sys, tempfile, threading, time
from concurrent.futures import ThreadPoolExecutor

from tlxlib import ROOT

# Seconds to wait after a TLC run before measuring tlx.
TLC_SETTLE_S = 1.0

# Explicit exit statuses of `bench.py ttfr` (see the module docstring).
EXIT_OK = 0
EXIT_ERRORS = 1
EXIT_INCOMPLETE = 2
EXIT_MISMATCH = 3

# Result kinds of a measured run (bench.tlx / tlc.parse) that end a complete
# search with a verdict, however the model behaves: a usable measurement.
COMPLETED = ("success", "safety failure", "deadlock", "property failure", "liveness failure",
             "init property failure", "postcondition failure")
# Severity of a model's outcome, worst last; the explicit exit status maps
# error > incomplete (skipped/timeout) > mismatch > completed.
RANK = {"completed": 0, "mismatch": 1, "skipped": 2, "timeout": 2, "error": 3}

# The phases of `-stats json` reported (key in "phases" -> short name).
PHASES = [
    ("front end (parse, sema, cfg)", "front"),
    ("pre-run", "prerun"),
    ("model build (lowering, C generation)", "lower"),
    ("JIT compile", "jit"),
    ("initial states", "init"),
    ("search", "search"),
    ("liveness", "live"),
    ("total", "total"),
]


def add_args(p):
    p.add_argument("--bin", action="append", required=True, help="NAME=PATH of a tlx binary (repeatable)")
    p.add_argument("--stats-bin", default="", help="the binary (NAME) whose losers get a -stats run (default: the last --bin)")
    p.add_argument("--models", default="", help="only these model ids (comma-separated)")
    p.add_argument("--add", default="", help="also these models without an oracle, as TLA::CFG (paths relative to the repository; comma-separated)")
    p.add_argument("--filter", default="", help="only model ids containing this")
    p.add_argument("--external", default="", help="also the external models of these manifest tiers (small,medium,...)")
    p.add_argument("--tlc-baseline", default="", help="TLC baseline (bench/history name or path) of this machine type")
    p.add_argument("--tlc-from", default="", help="an earlier ttfr file of this machine type: its TLC times (measured or from a "
                                                  "baseline) are taken first (source as recorded there, \"(earlier run)\" added)")
    p.add_argument("--tlc-fresh-below", type=float, default=30.0, help="measure TLC here when the baseline took less (s)")
    p.add_argument("--tlc-oracle-above", type=float, default=0,
                   help="a model without a baseline run whose oracle TLC time is at least this (s) takes the oracle's time (source "
                        "\"oracle\": measured with other runs in parallel, so not a same-machine time) instead of a run here (0: never)")
    p.add_argument("--tlc-timeout", type=int, default=600)
    p.add_argument("--cap-factor", type=float, default=2.0)
    p.add_argument("--cap-min", type=float, default=20.0)
    p.add_argument("--warm", type=int, default=2, help="warm runs per binary (1 when TLC takes >= 5 s)")
    p.add_argument("--once", default="", help="binaries (NAMEs) run once, without warm runs (a default tier without a native cache)")
    p.add_argument("--near", type=float, default=0.9, help="-stats run when tlx/TLC >= this")
    p.add_argument("--lanes", type=int, default=1)
    p.add_argument("--deadline-min", type=float, default=0,
                   help="start no model that could end after this many minutes (0: none): its expected time is its TLC time "
                        "(if measured here) plus the cap of every --once binary plus a tenth of the cap per other binary run")
    p.add_argument("--resume", action="store_true")
    p.add_argument("--out", default="bench/ttfr.json")


def add_report_args(p):
    p.add_argument("file", nargs="+", help="ttfr files; a later file's binaries replace an earlier one's for its models")
    p.add_argument("--markdown", action="store_true")
    p.add_argument("--near", type=float, default=0.9)


def models_of(a):
    from corpus import EXTERNAL_TIERS, external_models
    ext = []
    if a.external:
        tiers = [t for t in a.external.split(",") if t]
        ext = [m["id"] for m in external_models(tuple(t for t in tiers if t in EXTERNAL_TIERS))]
    d = bench_models.oracle(ext)
    keep = {k: m for k, m in d.items() if (m.get("tlc") or {}).get("result") or k in ext}
    custom = {}
    for x in a.add.split(","):
        if "::" in x:
            tla, cfg = (os.path.join(ROOT, p) for p in x.split("::", 1))
            custom[x] = {"id": x, "tla": tla, "cfg": cfg, "flags": [], "features": [], "tlc": {}}
    if a.models:
        only = set(a.models.split(","))
        keep = {k: m for k, m in keep.items() if k in only}
    keep.update(custom)
    if a.filter:
        keep = {k: m for k, m in keep.items() if a.filter in k}
    # (Not the fixed stress configurations, which have no oracle.)
    return {k: m for k, m in keep.items() if k not in bench_models.STRESS_MODELS or (m.get("tlc") or {}).get("result")}


def short(r):
    # `compatibility` is part of the evidence a caller (same()) reads to decide
    # whether a count difference is a reduction or a disagreement; dropping it
    # here made a default-mode reduction look like a TLC mismatch (HARNESS-19).
    return {k: r.get(k) for k in ("result", "distinct", "generated", "depth", "wall_s", "cpu_s", "exit", "compatibility")
            if r.get(k) is not None}


def run_kind(r):
    """The outcome bucket of one measured run: a verdict, an error, a cap
    cut-off, or incomplete evidence (a budget skip or an unparsed run)."""
    result = r.get("result")
    if result in COMPLETED:
        return "completed"
    if result in ("compile failure", "error"):
        return "error"
    if result == "timeout":
        return "timeout"
    return "skipped"


def bin_kind(b):
    """The outcome bucket of one binary's runs of a model: the worst of its
    runs, or a TLC disagreement when every run completed."""
    runs = [r for r in (b.get("cold"), b.get("warm")) if isinstance(r, dict)]
    if any(r.get("result") == "compile failure" for r in runs):
        # (A whole-model compiler fallback is a failed model even with a
        # correct verdict; the shared budget records it as a failure too.)
        return "error"
    kinds = [run_kind(r) for r in runs]
    worst = max(kinds, key=RANK.get) if kinds else "skipped"
    return "mismatch" if worst == "completed" and b.get("same") is False else worst


def outcome(ms, out):
    """Classify the scheduled models `ms` against the persisted ttfr `out`.

    Returns (status, counts): the explicit exit status and a count per kind.
    Every model of `ms` lands in exactly one bucket, read from the whole file
    (never the last sample), so an exception, a deadline skip, an all-timeout
    run, a budget refusal or a TLC disagreement stays visible. A model that
    was not scheduled is not counted.
    """
    errors = out.get("errors") or {}
    skipped = set(out.get("skipped_deadline") or [])
    models = out.get("models") or {}
    counts = {k: 0 for k in ("completed", "error", "skipped", "timeout", "mismatch", "missing")}
    for k in ms:
        rec = models.get(k)
        if k in errors:
            counts["error"] += 1
        elif k in skipped:
            counts["skipped"] += 1
        elif rec is None:
            counts["missing"] += 1
        else:
            kinds = [bin_kind(b) for b in (rec.get("bins") or {}).values()]
            counts[max(kinds, key=RANK.get) if kinds else "skipped"] += 1
    if counts["error"]:
        status = EXIT_ERRORS
    elif counts["skipped"] or counts["timeout"] or counts["missing"]:
        status = EXIT_INCOMPLETE
    elif counts["mismatch"]:
        status = EXIT_MISMATCH
    else:
        status = EXIT_OK
    return status, counts


def run(a):
    import bench
    # (Absolute paths: scratch models run in another directory.)
    bins = {n: os.path.abspath(p) for n, p in (b.split("=", 1) for b in a.bin)}
    for name, path in bins.items():
        if not os.path.exists(path):
            sys.exit(f"no binary {path} ({name})")
    stats_bin = a.stats_bin or list(bins)[-1]
    base = {}
    if a.tlc_baseline:
        _, runs = bench.load_tlc_baseline(a.tlc_baseline)
        base = {k: r for (k, w), r in runs.items() if w == 1}
    ms = models_of(a)
    if not ms:
        # (Status 0 requires a nonempty selection: an empty one is never PASS.)
        print("no models selected: check --models, --filter and --external", file=sys.stderr, flush=True)
        return EXIT_INCOMPLETE
    out = json.load(open(a.out)) if a.resume and os.path.exists(a.out) else {}
    out.setdefault("machine", bench_machine.machine_desc())
    out.setdefault("commit", bench_machine.git_commit())
    out.setdefault("date", time.strftime("%Y-%m-%d %H:%M:%S"))
    out["bins"] = bins
    out["lanes"] = a.lanes
    out.setdefault("models", {})
    lock = threading.Lock()
    start = time.time()

    def tlc_time(k, m):
        r = bench.tlc(m, 1, a.tlc_timeout)
        return {**r, "source": "canonical TLC baseline"}

    once = set(a.once.split(","))

    def expected(k):
        """A pessimistic estimate of the time model `k` takes (s)."""
        t = (base.get(k) or {}).get("wall_s") or (ms[k].get("tlc") or {}).get("wall_s") or a.cap_min
        cap = max(a.cap_factor * t, a.cap_min)
        fresh = t if t < a.tlc_fresh_below else 0
        return fresh + sum(cap if n in once else 0.1 * cap * (1 + a.warm) for n in bins)

    def one(k):
        # (A failure of one model is recorded; the others still run.)
        try:
            measure(k)
        except Exception as e:  # noqa: BLE001 - one model's failure must not end a cloud run
            with lock:
                out.setdefault("errors", {})[k] = f"{type(e).__name__}: {e}"
            print(f"error: {k}: {type(e).__name__}: {e}", flush=True)

    def measure(k):
        m = ms[k]
        if a.deadline_min and time.time() - start + expected(k) > a.deadline_min * 60:
            with lock:
                out.setdefault("skipped_deadline", []).append(k)
            print(f"skipped (deadline): {k}", flush=True)
            return
        t = tlc_time(k, m)
        tw = t.get("wall_s") if t.get("result") != "timeout" else None
        cap = max(a.cap_factor * (tw or a.tlc_timeout), a.cap_min)
        args = ["-workers", "1"] + bench_models.bench_flags(m)
        from benchmarking.bench_records import content_digest
        digest = content_digest(m)
        rec = {"tlc": t, "tlc_oracle": short(m.get("tlc") or {}), "cap_s": round(cap, 1), "bins": {}}
        for name, path in bins.items():
            cache = tempdirs.mkdtemp(prefix="tlx-ttfr-")
            try:
                env = {"TLX_CACHE": cache}
                cold = bench.tlx(m, args, cap, exe=path, env=env)
                bench.wait_builds(cache)
                once = name in a.once.split(",")
                warm = [] if once or cold.get("result") == "timeout" else [bench.tlx(m, args, cap, exe=path, env=env)
                                                                    for _ in range(a.warm if (tw or 99) < 5 else 1)]
                bench.wait_builds(cache)
                metadata = {"content_digest": digest, "tlc_version": bench_machine.tla2tools_info().get("revision")}
                r = {"cold": {**short(cold), **metadata}, "warm": {**short(bench.best(warm)), **metadata} if warm else {**short(cold), **metadata} if once else None, "warm_runs": [w.get("wall_s") for w in warm],
                     "same": all(same(x, t, m.get("tlc") or {}) for x in [cold, *warm] if x.get("result") != "timeout")}
                worst = max(x.get("wall_s") or cap for x in [cold, *warm])
                if name == stats_bin and tw and (cold.get("result") == "timeout" or worst >= a.near * tw):
                    sc = tempdirs.mkdtemp(prefix="tlx-ttfr-stats-")
                    sf = os.path.join(sc, "stats.json")
                    s = bench.tlx(m, [*args, "-stats", f"json={sf}"], cap, exe=path, env={"TLX_CACHE": sc})
                    bench.wait_builds(sc)
                    try:
                        ph = json.load(open(sf)).get("phases", {})
                        r["stats"] = {"wall_s": s.get("wall_s"), **{n: ph.get(key) for key, n in PHASES if ph.get(key) is not None},
                                      "C build": ph.get("C build -O2"), "C cache": ph.get("C build -O2 cache"),
                                      "native -O2 ready at": ph.get("native -O2 ready at (since start)"),
                                      "pre-run reused": ph.get("pre-run reused as the search"),
                                      "restarts": ph.get("restarts (layout miss)"),
                                      "fallbacks": ph.get("action runs fell back to the interpreter")}
                    except (OSError, ValueError) as e:
                        r["stats"] = {"error": str(e)}
                    shutil.rmtree(sc, ignore_errors=True)
                rec["bins"][name] = r
            finally:
                shutil.rmtree(cache, ignore_errors=True)
        with lock:
            out["models"][k] = rec
            # (A --resume retry that now succeeds must not keep an earlier
            # error or deadline skip for the same model in the outcome.)
            (out.get("errors") or {}).pop(k, None)
            if k in (out.get("skipped_deadline") or []):
                out["skipped_deadline"].remove(k)
            tmp = a.out + ".tmp"
            json.dump(out, open(tmp, "w"), indent=1, sort_keys=True)
            os.replace(tmp, a.out)
            cells = " ".join(f"{n} {ratio(rec, n, 'cold')}/{ratio(rec, n, 'warm')}" for n in bins)
            print(f"[{len(out['models'])}/{len(ms)} {time.time() - start:6.0f}s] {k:<62} TLC {t.get('wall_s')} ({t['source']}) {cells}", flush=True)

    def key(k):
        # (Models without an oracle (--add) first: their time is unknown.)
        if "::" in k:
            return -1
        b = base.get(k) or {}
        return b.get("wall_s") or (ms[k].get("tlc") or {}).get("wall_s") or 1e9

    todo = sorted((k for k in ms if not (a.resume and k in out["models"])), key=key)
    print(f"{len(todo)} models, binaries {bins}, {a.lanes} lane(s)", flush=True)
    os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
    with ThreadPoolExecutor(max(1, a.lanes)) as ex:
        list(ex.map(one, todo))
    out["date_end"] = time.strftime("%Y-%m-%d %H:%M:%S")
    # (Retain the file before mapping the outcome to a status: a cut-off run
    # keeps what it measured even when it exits non-zero.)
    status, counts = outcome(ms, out)
    out["outcome"] = {"status": status, **counts}
    json.dump(out, open(a.out, "w"), indent=1, sort_keys=True)
    from benchmarking.bench_records import ingest, generate
    ingest(a.out)
    generate(refresh=True)
    report(out, a.near, False)
    if status != EXIT_OK:
        bad = ", ".join(f"{k} {counts[k]}" for k in counts if counts[k])
        print(f"ttfr outcome: incomplete or failed ({bad}) -> exit {status}", flush=True)
    return status


def same(r, t, oracle):
    """Whether tlx result `r` agrees with TLC's (`t` measured here, else the
    oracle): the result kind and, only under -tlc, the distinct states of a complete
    search (default-mode reductions and simulation may change counts)."""
    ref = t if t.get("result") not in (None, "timeout") else oracle
    if not ref.get("result"):
        return True
    if r.get("result") != ref.get("result"):
        return False
    return (r.get("compatibility") is False or r.get("result") != "success"
            or not ref.get("distinct") or r.get("distinct") == ref.get("distinct"))


def ratio(rec, name, which):
    """tlx / TLC wall time of binary `name` (`cold` or `warm`), or None."""
    t = rec["tlc"]
    r = (rec["bins"].get(name) or {}).get(which) or {}
    if t.get("result") == "timeout" or not t.get("wall_s"):
        return None
    if r.get("result") == "timeout":
        return math.inf
    return round(r["wall_s"] / t["wall_s"], 3) if r.get("wall_s") else None


def report(h, near=0.9, markdown=False):
    ms = h["models"]
    bins = list(h["bins"])
    lines = [f"{len(ms)} models; {h.get('machine')}; commit {h.get('commit')}; {h.get('date')} .. {h.get('date_end', '?')}; "
             f"lanes {h.get('lanes')}"]
    buckets = [(0, 0.25), (0.25, 0.5), (0.5, 0.9), (0.9, 1.0), (1.0, 1.5), (1.5, 3), (3, math.inf)]
    head = ("| binary | runs | geomean tlx/TLC (completed runs) | " + " | ".join(f"{lo:g}-{hi:g}" if hi != math.inf else f">={lo:g}" for lo, hi in buckets)
            + " | cut off (> cap) | slower than TLC |")
    lines += ["", "Distribution of tlx/TLC end-to-end wall time (lower is better; TLC 1w = 1; a run cut off at the cap "
              "max(cap factor x TLC, minimum) is slower than that):", "", head,
              "|" + "---|" * (head.count("|") - 1)]
    for n in bins:
        for which in ("cold", "warm"):
            rs = [r for r in (ratio(rec, n, which) for rec in ms.values()) if r is not None]
            fin = [r for r in rs if r != math.inf]
            g = math.exp(sum(map(math.log, fin)) / len(fin)) if fin else float("nan")
            cells = " | ".join(str(sum(lo <= r < hi for r in fin)) for lo, hi in buckets)
            lines.append(f"| {n} {which} | {len(rs)} | {g:.3f} | {cells} | {len(rs) - len(fin)} | {sum(r >= 1 for r in rs)} |")
    for n in bins:
        diff = [k for k, rec in ms.items() if (rec["bins"].get(n) or {}).get("same") is False]
        lines.append(f"\n{n}: results differ from TLC's (result kind / distinct states): {len(diff)} {' '.join(diff)}")
    for n in bins:
        bad = sorted(((max(ratio(rec, n, "cold") or 0, ratio(rec, n, "warm") or 0), k) for k, rec in ms.items()), reverse=True)
        bad = [(r, k) for r, k in bad if r >= near]
        lines += ["", f"{n}: {len(bad)} models with tlx/TLC >= {near} (cold or warm):", ""]
        if not bad:
            continue
        lines += ["| model | TLC s | src | cold s | warm s | cold/TLC | warm/TLC | phases (cold, -stats) |", "|---|---:|---|---:|---:|---:|---:|---|"]
        for r, k in bad:
            rec = ms[k]
            b = rec["bins"][n]
            st = b.get("stats") or {}
            ph = ", ".join(f"{p} {st[p]:.2f}" for _, p in PHASES if isinstance(st.get(p), (int, float)) and st[p] >= 0.01)
            if st.get("pre-run reused"):
                ph += ", pre-run reused"
            if st.get("restarts"):
                ph += f", {st['restarts']} restarts"
            if st.get("fallbacks"):
                ph += f", {st['fallbacks']} interpreter fallbacks"
            w = (b.get("warm") or {})
            lines.append(f"| {k} | {rec['tlc'].get('wall_s')} | {rec['tlc'].get('source')} | {fmt(b['cold'])} | {fmt(w)} | "
                         f"{ratio(rec, n, 'cold')} | {ratio(rec, n, 'warm')} | {ph} |")
    text = "\n".join(lines)
    if not markdown:
        text = "\n".join(l.strip("| ").replace(" | ", "  ") for l in text.splitlines() if not l.startswith("|---"))
    print(text)


def fmt(r):
    if not r:
        return "-"
    return "timeout" if r.get("result") == "timeout" else f"{r.get('wall_s')}"


def merged(files):
    """The ttfr files `files` as one: per model the TLC time of the first
    file that has the model, and per binary the runs of the last."""
    hs = [json.load(open(f if os.path.exists(f) else os.path.join(ROOT, f))) for f in files]
    out = {**hs[0], "models": {}, "bins": {}}
    for h in hs:
        out["bins"].update(h["bins"])
        for k, rec in h["models"].items():
            cur = out["models"].setdefault(k, {**rec, "bins": {}})
            cur["bins"].update(rec["bins"])
    if len(hs) > 1:
        out["commit"] = " + ".join(str(h.get("commit")) for h in hs)
        out["date_end"] = hs[-1].get("date_end", "?")
    return out


def run_report(a):
    from benchmarking.bench_records import ingest, generate
    for filename in a.file:
        ingest(filename if os.path.exists(filename) else os.path.join(ROOT, filename))
    generate(refresh=True)
    print("generated docs/performance.md")
