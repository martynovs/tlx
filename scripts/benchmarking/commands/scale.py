"""`bench.py scale` and `bench.py scale-report`: many-core scaling on large
models.

`benchmarking/cli.py` owns the parser and `scripts/bench.py` dispatches; the pass
and its report live here (CODE-03), reaching the helpers that stay in the entry
point (`bench.tlx`, `bench.build_release`, `bench.read_json`,
`bench.history_path`, `bench.HISTORY`, `bench.WARMUP_ENV`, `bench.tlc_measured`,
`bench.load_tlc_baseline`) through it, the way the other passes do (HARNESS-03).
"""
import json, os, statistics, sys, tempfile, time

from benchmarking import machine as bench_machine, models as bench_models
from benchmarking.commands.tlcbase import TLC_MS_PROP
from benchmarking.report import scale_report
from tlxlib import ROOT

# --- Many-core scaling (scale, scale-report) ---------------------------------

SCALE_KIND = "scale"



def scale_runs(rs):
    """The record of repeated tlx runs at one worker count: the aggregate (the
    times of `rs` and the result, counts and mode contract of its last run, as
    `scale_report` has always read them), `samples` with every run's own mode
    contract, and `problems`: the repeats that disagree with the last one that
    finished, so an earlier failure is not erased by a later success. The
    aggregate keeps the last run's `compatibility` and `tier` too, so a reduced
    default-mode count (compatibility False) is not later read as a
    TLC-agreeing one (HARNESS-19, HARNESS-21)."""
    from benchmarking.bench_suite import UNFINISHED, sample_record
    walls = [r["wall_s"] for r in rs]
    last = rs[-1]
    rec = {"walls": walls, "min": min(walls), "median": statistics.median(walls), "cpu": [r.get("cpu_s") for r in rs],
           "maxrss_mib": [r.get("maxrss_mib") for r in rs],
           "samples": [sample_record(r) for r in rs],
           **{x: last.get(x) for x in ("result", "distinct", "generated", "depth", "compatibility", "tier")}}
    problems = []
    # The last repeat that finished with a verdict is the reference the others
    # must agree with; a repeat with no verdict of its own (UNFINISHED) is
    # neither the reference nor a disagreement, and one that ended in a checker
    # error is a disagreement with the expectation, never the expectation itself.
    ref = next((r for r in reversed(rs) if r.get("result") not in UNFINISHED and r.get("result") != "error"), None)
    for i, r in enumerate(rs, 1):
        if ref is None or r is ref or r.get("result") in UNFINISHED:
            continue
        if r.get("result") != ref.get("result"):
            problems.append(f"sample {i}: result {r.get('result')!r}, expected {ref.get('result')!r}")
        elif ref.get("result") == "success" and r.get("distinct") != ref.get("distinct"):
            problems.append(f"sample {i}: distinct {r.get('distinct')}, expected {ref.get('distinct')}")
    if problems:
        rec["problems"] = problems
    return rec



def cmd_scale(a):
    import bench
    with bench.build_release("tlx-scale-") as binary:
        run_scale(a, binary)



def run_scale(a, binary):
    import bench
    from tlc import JVM_OPTS
    cores = os.cpu_count() or 1
    ws = bench_machine.scale_workers(a.workers, cores)
    tws = [] if a.no_tlc else bench_machine.scale_workers(a.tlc_workers or a.workers, cores)
    ms = bench_models.scale_models([k.strip() for k in a.models.split(",") if k.strip()] if a.models else bench_models.SCALE_DEFAULT)
    tlc1 = set(ms) if a.tlc_1w == "all" else set() if a.tlc_1w == "none" else {k for k in a.tlc_1w.split(",") if k}
    if bad := tlc1 - set(ms):
        sys.exit(f"--tlc-1w: not among the models: {', '.join(sorted(bad))}")
    if over := [w for w in ws + tws if w > cores]:
        print(f"warning: worker counts {over} exceed the {cores} CPUs", file=sys.stderr)
    mem_opts = bench_machine.tlc_mem_opts(a.tlc_mem)
    out = a.out or os.path.join(bench.HISTORY, f"scale-{bench_machine.git_commit()}.json")
    h = bench.read_json(out, {})
    if h and h.get("kind") != SCALE_KIND:
        sys.exit(f"{out} is not a scaling file")
    h.update({"kind": SCALE_KIND, "date": h.get("date") or time.strftime("%Y-%m-%d %H:%M:%S"), "machine": bench_machine.machine_desc(),
              "bench_machine": os.environ.get("TLX_BENCH_MACHINE", ""), "commit": bench_machine.git_commit(), "topology": bench_machine.cpu_topology(),
              "mem_gib": round(bench_machine.mem_bytes() / (1 << 30), 1), "tla2tools": bench_machine.tla2tools_info(),
              "jvm": {**bench_machine.jvm_info(), "options": JVM_OPTS + [TLC_MS_PROP] + mem_opts}, "workers": ws, "tlc_workers": tws,
              "repeat": a.repeat, "repeat_1w": a.repeat_1w, "timeout_s": a.timeout, "tlc_timeout_s": a.tlc_timeout or a.timeout,
              "tlx_args": "-compile (default tiering: JIT, then C -O2), warm C cache"})
    models = h.setdefault("models", {})

    def save():
        h["date_end"] = time.strftime("%Y-%m-%d %H:%M:%S")
        os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
        with open(out, "w") as stream:
                    json.dump(h, stream, indent=1, sort_keys=True)
    base = bench.load_tlc_baseline(a.tlc_baseline)[1] if a.tlc_baseline else {}
    print(f"scaling on {h['machine']}: {len(ms)} models, tlx workers {ws}, TLC workers {tws} (1w: {sorted(tlc1) if 1 in tws else '-'}) -> "
          f"{os.path.relpath(out, ROOT)}", flush=True)
    for k, m in ms.items():
        rec = models.setdefault(k, {})
        rec.update({"tla": bench_models.short_path(m["tla"]), "cfg": bench_models.short_path(m["cfg"])})
        ref = [r.get("distinct") for (bk, _), r in base.items() if bk == k and r.get("distinct")]
        if ref:
            rec["reference"] = {"distinct": ref[0], "source": os.path.basename(bench.history_path(a.tlc_baseline))}
        tlxr, tlcr = rec.setdefault("tlx", {}), rec.setdefault("tlc", {})
        if "warmup" not in rec or a.force:
            # Builds the C library (and checks the counts once at the most workers).
            rec["warmup"] = bench.tlx(m, ["-compile", "-workers", str(max(ws))], a.timeout, exe=binary, env=bench.WARMUP_ENV)
            print(f"  {k:<60} warm-up {max(ws)}w {rec['warmup'].get('result')} {rec['warmup'].get('distinct')} "
                  f"wall {rec['warmup']['wall_s']:.2f}s", flush=True)
            save()
        for w in ws:
            if str(w) in tlxr and not a.force:
                continue
            rs = []
            for _ in range(max(1, a.repeat_1w if w == 1 else a.repeat)):
                rs.append(bench.tlx(m, ["-compile", "-workers", str(w)], a.timeout, exe=binary))
                if rs[-1].get("result") == "timeout":
                    break
            tlxr[str(w)] = r = {**scale_runs(rs), "load": round(os.getloadavg()[0], 2)}
            if a.stats and w > 1:
                with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as f:
                    path = f.name
                bench.tlx(m, ["-compile", "-workers", str(w), "-stats", f"json={path}"], a.timeout, exe=binary)
                try:
                    r["stats"] = bench.read_json(path)
                except (OSError, ValueError):
                    pass
                os.unlink(path)
            print(f"  {k:<60} tlx {w:>3}w {r['result']} {r['distinct']} min {r['min']:.2f}s cpu {min(c or 0 for c in r['cpu']):.2f}s", flush=True)
            save()
        for w in tws:
            if w == 1 and k not in tlc1:
                continue
            tlcr[str(w)] = r = bench.tlc_measured(m, w, a.tlc_timeout or a.timeout, jvm_extra=mem_opts)
            print(f"  {k:<60} TLC {w:>3}w {r['result']} {r.get('distinct', '-')} wall {r['wall_s']:.2f}s cpu {r['cpu_s']:.2f}s "
                  f"rss {r['maxrss_mib']:.0f}M", flush=True)
            save()
    print(f"recorded in {os.path.relpath(out, ROOT)}\n")
    print("\n".join(scale_report(h)))



def cmd_scale_report(a):
    import bench
    h = bench.read_json(bench.history_path(a.file))
    if h.get("kind") != SCALE_KIND:
        sys.exit(f"{a.file} is not a scaling file (bench.py scale)")
    topo = h.get("topology") or {}
    print(f"{h.get('machine')}; {topo.get('sockets', '?')} socket(s) x {topo.get('cores_per_socket', '?')} cores x "
          f"{topo.get('threads_per_core', '?')} threads, {topo.get('numa_nodes', '?')} NUMA node(s), {h.get('mem_gib')} GiB; "
          f"commit {h.get('commit')}, {h.get('date')} .. {h.get('date_end')}\n")
    if a.tlc_baseline:
        # TLC runs this file lacks, from a baseline (meaningful on the same machine type).
        _, base = bench.load_tlc_baseline(a.tlc_baseline)
        filled = []
        for k, rec in h["models"].items():
            for (bk, w), r in base.items():
                if bk == k and str(w) not in rec.setdefault("tlc", {}):
                    rec["tlc"][str(w)] = r
                    filled.append(f"{k} {w}w")
        print(f"TLC runs from {os.path.basename(bench.history_path(a.tlc_baseline))}: {', '.join(filled) or 'none'}\n")
    print("\n".join(scale_report(h)))
