#!/usr/bin/env python3
"""Where the time per state goes as the worker count grows (bench.py profile).

For each model and worker count: one run measured (wall, user and system
CPU, peak RSS) under `perf stat` when Linux perf works (cycles,
instructions, cache misses, context switches, page faults; a counter the
machine does not expose is recorded as missing), and one run with
`-stats json` (the time per category, per action and path). At the worker
counts of `--record` (among those of `--stats`), the `-stats` run is also sampled with `perf record`
(flat profile, no call graphs: release builds have no frame pointers); the
symbols whose samples per state grew the most between the lowest and the
highest recorded worker count are annotated (`perf annotate`, the hottest
instructions), which shows atomic read-modify-writes and lock words that
bounce between cores. `--c2c MODEL@W` adds a `perf c2c` run (false sharing)
where the machine supports it.

On a cloud instance (TLX_CLOUD_HOST) perf is installed with apt and
perf_event_paranoid lowered when missing; elsewhere the runs go on without
perf. Everything is written under --dir (default bench/profile): the
summary JSON, the perf reports and annotations as text. The per-state
numbers are per distinct state (the work is the same at every worker count,
which the counts check confirms).

Model ids: an Examples manifest model, bench/<name> (bench/scaling), or
ext/<repo>/<name>; suffix `@interp` runs it without -compile.
"""

from benchmarking import models as bench_models, machine as bench_machine

from benchmarking import bench_budget

import argparse
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import time

from benchmarking import measurement
from tlxlib import ROOT, run_proc
from tlc import compile_fallback

PERF_EVENTS = ["task-clock", "context-switches", "cpu-migrations", "page-faults", "cycles", "instructions", "cache-references",
               "cache-misses", "branch-misses"]


def env_override(item):
    key, equal, _ = item.partition("=")
    if not key or not equal:
        raise argparse.ArgumentTypeError(f"--env expects K=V, got {item!r}")
    return item


def add_args(p):
    p.add_argument("--models", required=True, help="comma-separated ids (suffix @interp: without -compile)")
    p.add_argument("--workers", default="1,4,8,16", help="tlx -workers values (max: the number of CPUs)")
    p.add_argument("--record", default="", help="worker counts whose -stats run is also sampled with perf record (default: lowest and highest)")
    p.add_argument("--freq", type=int, default=499, help="perf record sampling frequency (Hz)")
    p.add_argument("--c2c", default="", help="MODEL@W[,MODEL@W]: also a perf c2c run of these (false sharing; best effort)")
    p.add_argument("--bin", default="", help="tlx binary (default: build target/release/tlx)")
    p.add_argument("--bin-rev", default="", help="revision of --bin (unknown when omitted for an external binary)")
    p.add_argument("--stats", default="", help="worker counts with a -stats run (default: all; none: no -stats runs)")
    p.add_argument("--timeout", type=int, default=1800)
    p.add_argument("--extra", default="", help="extra tlx arguments (for example --extra=-tlc for count-compatible profiles)")
    p.add_argument("--env", action="append", type=env_override, default=[], metavar="K=V", help="environment override for every pass (repeatable)")
    p.add_argument("--dir", default=os.path.join("bench", "profile"))


def sh(cmd, timeout=600, **kw):
    """Run a helper command: (returncode, output); never raises."""
    remaining = bench_budget.current().remaining()
    if remaining <= 0:
        return -1, "skipped: benchmark deadline"
    timeout = min(timeout, remaining)
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, **kw)
        output = p.stdout + p.stderr
        if 'check' in cmd and (fallback := compile_fallback(output)):
            bench_budget.fail(cmd[cmd.index('check') + 1], fallback)
            return 1, output
        return p.returncode, output
    except (OSError, subprocess.TimeoutExpired) as e:
        return -1, str(e)


def ensure_perf():
    """A working perf binary (installed on a cloud host), or None."""
    def works():
        exe = shutil.which("perf")
        if not exe:
            return None
        rc, out = sh([exe, "stat", "-e", "task-clock", "--", "true"], timeout=60)
        return exe if rc == 0 else None
    if (exe := works()) or not os.environ.get("TLX_CLOUD_HOST"):
        return exe
    rel = os.uname().release
    env = {**os.environ, "DEBIAN_FRONTEND": "noninteractive"}
    for pkgs in (["linux-tools-common", f"linux-tools-{rel}"], ["linux-tools-common", "linux-tools-generic"]):
        rc, out = sh(["sudo", "-n", "-E", "apt-get", "-o", "DPkg::Lock::Timeout=600", "install", "-y", "-qq", *pkgs], timeout=900, env=env)
        print(f"  perf: apt-get install {' '.join(pkgs)}: exit {rc}", flush=True)
        if rc == 0:
            break
    sh(["sudo", "-n", "sysctl", "-w", "kernel.perf_event_paranoid=-1", "kernel.kptr_restrict=0"])
    exe = works()
    if not exe:
        # linux-tools-generic installs perf under /usr/lib/linux-tools/<version>/.
        for d in sorted(os.listdir("/usr/lib/linux-tools")) if os.path.isdir("/usr/lib/linux-tools") else []:
            cand = os.path.join("/usr/lib/linux-tools", d, "perf")
            if os.access(cand, os.X_OK) and sh([cand, "stat", "-e", "task-clock", "--", "true"], timeout=60)[0] == 0:
                return cand
    return exe


def parse_perf_stat(text):
    """`perf stat -x,` output: {event: count}; None for an event the machine
    does not count ("<not supported>", "<not counted>")."""
    out = {}
    for line in text.splitlines():
        f = line.split(",")
        if len(f) < 3 or line.startswith("#"):
            continue
        value, event = f[0].strip(), f[2].strip()
        # (Event names may carry a modifier, e.g. "cycles:u".)
        event = event.split(":")[0]
        if not event:
            continue
        try:
            out[event] = float(value)
        except ValueError:
            out[event] = None
    return out


def parse_perf_report(text):
    """`perf report --stdio --sort dso,sym` (no children): [(percent, samples,
    dso, symbol)] by decreasing share."""
    rows = []
    for line in text.splitlines():
        m = re.match(r"\s*([\d.]+)%\s+(\d+)\s+(\S+)\s+\[[.kgu]\]\s+(.+?)\s*$", line)
        if m:
            rows.append((float(m.group(1)), int(m.group(2)), m.group(3), m.group(4)))
    return rows


def growth(lo, hi, n_lo, n_hi, top=8):
    """The symbols whose samples per state grew the most from the run `lo`
    to the run `hi` (parsed reports; `n_*` their states): [(symbol, samples
    per million states lo, hi)], largest absolute growth first."""
    def per(rows, n):
        d = {}
        for _, samples, dso, sym in rows:
            d[(dso, sym)] = d.get((dso, sym), 0) + samples * 1e6 / max(n, 1)
        return d
    a, b = per(lo, n_lo), per(hi, n_hi)
    g = sorted(((b[k] - a.get(k, 0.0), k) for k in b), reverse=True)
    return [(k[1], round(a.get(k, 0.0), 2), round(b[k], 2)) for d, k in g[:top] if d > 0]


def resolve(ids):
    """{id: (model, extra tlx args)} for `ids` (see the module)."""
    out = {}
    for full in ids:
        k, _, mode = full.partition("@")
        if mode not in ("", "interp"):
            sys.exit(f"unknown mode @{mode} of {full} (only @interp)")
        if k.startswith("ext/"):
            d = bench_models.oracle([k])
            if k not in d:
                sys.exit(f"unknown model {k}")
            m = d[k]
        elif k.startswith("bench/") and k in (fixed := bench_models.stress_models([k])):
            m = fixed[k]
        else:
            m = bench_models.scale_models([k])[k]
        out[full] = (m, [] if mode == "interp" else ["-compile"])
    return out


@bench_budget.model_pass
def run_one(m, args, timeout, exe, env=None, wrap=()):
    """One tlx run (wrapped by `wrap`, e.g. perf stat): the parsed result,
    wall, user and system CPU, peak RSS."""
    import bench
    exe = measurement.binary_path(exe)
    with measurement.staged_run(m, "tlx-prof-") as (m, cwd, _):
        cmd = [*wrap, *bench.tlx_cmd(m, args + bench_models.bench_flags(m), exe)]
        p = run_proc(cmd, timeout, cwd=cwd, env=measurement.measured_env(env), rusage=True, raise_timeout=False, start_new_session=True)
        # (The child's own deadline result, not "any nonzero exit that took
        # long": a model that fails at its own pace is not a timeout.)
        r = measurement.result_of(p, timeout)
    ru = p.rusage
    r.update(user_s=round(ru.ru_utime, 3), sys_s=round(ru.ru_stime, 3), cpu_s=measurement.cpu_seconds(ru),
             maxrss_mib=measurement.rss_mib(ru), nvcsw=ru.ru_nvcsw, nivcsw=ru.ru_nivcsw)
    return {k: v for k, v in r.items() if k not in ("stdout",)}


def summarize_stats(s):
    """The per-category and per-path time of a -stats JSON, per distinct
    state (ns), and the worker wait times."""
    n = max(s.get("counters", {}).get("units") or 0, 1)
    ns = lambda v: round(v * 1e9 / n)
    paths = {}
    calls = {}
    for a in s.get("actions", []):
        for p, v in a.get("est_s", {}).items():
            paths[p] = paths.get(p, 0.0) + v
        for p, v in a.get("calls", {}).items():
            calls[p] = calls.get(p, 0) + v
    ws = s.get("workers", [])
    return {"ns_per_state": {"total": ns(s.get("units_time_est_s", 0.0)), **{c: ns(v) for c, v in s.get("time_est_s", {}).items() if v},
                             **{"action_" + p: ns(v) for p, v in paths.items() if v}},
            "calls": calls, "barrier_s": round(sum(w.get("barrier_s", 0) for w in ws), 2), "idle_s": round(sum(w.get("idle_s", 0) for w in ws), 2)}


def run(a):
    import bench
    env = {}
    for item in a.env:
        key, equal, value = item.partition("=")
        if not key or not equal:
            raise ValueError(f"--env expects K=V, got {item!r}")
        env[key] = value
    cores = os.cpu_count() or 1
    ws = bench_machine.scale_workers(a.workers, cores)
    rec_ws = set(bench_machine.scale_workers(a.record, cores)) if a.record else {min(ws), max(ws)}
    stats_ws = set(ws) if not a.stats else set() if a.stats == "none" else set(bench_machine.scale_workers(a.stats, cores))
    rec_ws &= stats_ws
    ms = resolve([k.strip() for k in a.models.split(",") if k.strip()])
    c2c = {}
    for x in filter(None, (y.strip() for y in a.c2c.split(","))):
        k, _, w = x.rpartition("@")
        c2c.setdefault(k, set()).add(int(w))
    outdir = os.path.join(ROOT, a.dir)
    os.makedirs(outdir, exist_ok=True)
    perf = ensure_perf()
    print(f"profile on {bench_machine.machine_desc()}: {len(ms)} models, workers {ws}, perf record at {sorted(rec_ws)}, perf {perf or 'unavailable'}",
          flush=True)
    if perf:
        print(sh([perf, "--version"])[1].strip(), flush=True)
    runner_commit = bench_machine.git_commit()
    measured_commit = getattr(a, "bin_rev", "") or (None if a.bin else runner_commit)
    if getattr(a, "bin_rev", "") and not a.bin:
        raise ValueError("--bin-rev requires --bin")
    binary = {"path": a.bin or "target/release/tlx", "revision": measured_commit,
              "revision_source": "explicit" if getattr(a, "bin_rev", "") else "unknown" if a.bin else "built checkout"}
    out = os.path.join(outdir, f"profile-{runner_commit}.json")
    h = {}
    if os.path.exists(out):
        with open(out) as stream:
            h = json.load(stream)
    if h.get("models") and h.get("binary") != binary:
        raise ValueError("profile output contains another or unknown binary; choose a fresh --dir")
    if h.get("models") and (h.get("env") or {}) != env:
        raise ValueError("profile output contains another environment; choose a fresh --dir")
    h.update({"kind": "profile", "diagnostic": True, "machine": bench_machine.machine_desc(), "bench_machine": os.environ.get("TLX_BENCH_MACHINE", ""),
              "commit": measured_commit, "runner_commit": runner_commit, "binary": binary, "env": env,
              "topology": bench_machine.cpu_topology(), "workers": ws, "perf": bool(perf),
              "date": h.get("date") or time.strftime("%Y-%m-%d %H:%M:%S")})
    models = h.setdefault("models", {})

    def save():
        bench_budget.record(h)
        h["date_end"] = time.strftime("%Y-%m-%d %H:%M:%S")
        with open(out, "w") as stream:
            json.dump(h, stream, indent=1, sort_keys=True)

    ctx = bench.build_release("tlx-prof-") if not a.bin else None
    exe = os.path.abspath(a.bin) if a.bin else ctx.__enter__()
    try:
        for k, (m, margs) in ms.items():
            tag = re.sub(r"[^A-Za-z0-9_.-]+", "_", k)
            compiled = bool(margs)
            margs = (margs or bench.interp_args(exe)) + shlex.split(getattr(a, "extra", ""))
            rec = models.setdefault(k, {"args": margs, "runs": {}})
            if compiled:
                r = run_one(m, margs + ["-workers", str(max(ws))], a.timeout, exe, env={**env, **bench.WARMUP_ENV})
                print(f"  {k:<56} warm-up {max(ws)}w {r.get('result')} {r.get('distinct')} wall {r['wall_s']:.2f}s", flush=True)
            reports = {}
            # (@interp: -interp since the compiled tier is the default; an
            # older binary has the interpreter as its default.)
            for w in ws:
                args = margs + ["-workers", str(w)]
                r = {}
                if perf:
                    sf = os.path.join(outdir, f"{tag}-{w}w.perfstat")
                    r = run_one(m, args, a.timeout, exe, env=env, wrap=[perf, "stat", "-x,", "-o", sf, "-e", ",".join(PERF_EVENTS), "--"])
                    try:
                        with open(sf) as stream:
                            r["perf"] = parse_perf_stat(stream.read())
                    except OSError:
                        pass
                else:
                    r = run_one(m, args, a.timeout, exe, env=env)
                if r.get('result') in ('skipped', 'timeout'):
                    rec['runs'][str(w)] = r
                    print(f"  {k:<56} {w:>3}w {r['result']}: {r.get('reason', 'sample timeout')}", flush=True)
                    save()
                    continue
                n = r.get("distinct") or 0
                pc = r.get("perf") or {}
                if n and pc:
                    r["per_state"] = {e: round(v / n, 1) for e, v in pc.items() if v is not None and e != "task-clock"}
                    if pc.get("cycles") and pc.get("instructions"):
                        r["ipc"] = round(pc["instructions"] / pc["cycles"], 3)
                    if pc.get("cycles") and pc.get("task-clock"):
                        # (perf reports task-clock in milliseconds.)
                        r["ghz"] = round(pc["cycles"] / (pc["task-clock"] * 1e6), 3)
                if n:
                    r["cpu_us_per_state"] = round(r["cpu_s"] * 1e6 / n, 3)
                if w in stats_ws:
                    # Keep full region/layout attribution alongside the aggregate.
                    path = os.path.join(outdir, f"{tag}-{w}w.stats.json")
                    wrap = []
                    data = os.path.join(outdir, f"{tag}-{w}w.perf.data")
                    if perf and w in rec_ws:
                        wrap = [perf, "record", "-q", "-F", str(a.freq), "-o", data, "--"]
                    s = run_one(m, args + ["-stats", f"json={path}"], a.timeout, exe, env=env, wrap=wrap)
                    try:
                        with open(path) as stream:
                            r["stats"] = summarize_stats(json.load(stream))
                    except (OSError, ValueError):
                        pass
                    if wrap and os.path.exists(data):
                        rc, txt = sh([perf, "report", "-i", data, "--stdio", "-n", "--no-children", "--sort", "dso,sym", "--percent-limit", "0.2"],
                                     timeout=900)
                        with open(os.path.join(outdir, f"{tag}-{w}w.report.txt"), "w") as out:
                            out.write(txt)
                        reports[w] = (parse_perf_report(txt), s.get("distinct") or n, data)
                        r["top"] = [(p, sym) for p, _, _, sym in reports[w][0][:25]]
                if k in c2c and w in c2c[k] and perf:
                    cdata = os.path.join(outdir, f"{tag}-{w}w.c2c.data")
                    rc, txt = sh([perf, "c2c", "record", "-o", cdata, "--", *bench.tlx_cmd(m, args, exe)], timeout=a.timeout,
                                 env=measurement.measured_env(env))
                    if rc == 0:
                        rc, txt = sh([perf, "c2c", "report", "-i", cdata, "--stdio", "--full-symbols"], timeout=900)
                    with open(os.path.join(outdir, f"{tag}-{w}w.c2c.txt"), "w") as out:
                        out.write(txt[:2_000_000])
                    r["c2c"] = rc == 0
                    if os.path.exists(cdata):
                        os.unlink(cdata)
                rec["runs"][str(w)] = r
                st = r.get("stats", {}).get("ns_per_state", {})
                print(f"  {k:<56} {w:>3}w {r.get('result')} {n} wall {r['wall_s']:.2f}s cpu {r['cpu_s']:.2f}s (sys {r['sys_s']:.2f}) "
                      f"{r.get('cpu_us_per_state', '-')}us/state ipc {r.get('ipc', '-')} ghz {r.get('ghz', '-')} "
                      f"ns/state {' '.join(f'{c}={v}' for c, v in st.items())}", flush=True)
                save()
            if len(reports) >= 2:
                lo, hi = min(reports), max(reports)
                g = growth(reports[lo][0], reports[hi][0], reports[lo][1], reports[hi][1])
                rec["growth"] = {"from": lo, "to": hi, "symbols": g}
                print(f"  {k}: samples per million states {lo}w -> {hi}w:", flush=True)
                for sym, x, y in g:
                    print(f"    {x:10.1f} -> {y:10.1f}  {sym[:140]}", flush=True)
                with open(os.path.join(outdir, f"{tag}-{hi}w.annotate.txt"), "w") as f:
                    for sym, _, _ in g[:5]:
                        rc, txt = sh([perf, "annotate", "-i", reports[hi][2], "--stdio", "-s", sym], timeout=600)
                        hot = [l for l in txt.splitlines() if re.match(r"\s*([1-9]\d*\.\d+|0\.[5-9]\d*)\s*:", l)]
                        f.write(f"==== {sym}\n" + "\n".join(hot[:80]) + "\n\n")
            for w, (_, _, data) in reports.items():
                if os.path.exists(data):
                    os.unlink(data)
            save()
    finally:
        if ctx:
            ctx.__exit__(None, None, None)
    from benchmarking.bench_records import ingest
    ingest(out)
    print(f"recorded in {os.path.relpath(out, ROOT)}")
    print("\n".join(report(h)))


def report(h):
    """Per model: the per-state CPU and counters by worker count, relative to
    the fewest workers."""
    lines = []
    for k, rec in h.get("models", {}).items():
        runs = rec.get("runs", {})
        if not runs:
            continue
        ws = sorted(runs, key=int)
        base = runs[ws[0]]
        lines.append(f"{k}:")
        lines.append(f"  {'w':>4} {'wall s':>8} {'cpu s':>8} {'sys s':>7} {'us/state':>9} {'x' + ws[0] + 'w':>6} {'eff':>5} {'ipc':>5} {'GHz':>5} "
                     f"{'instr/st':>9} {'cyc/st':>8} {'cmiss/st':>8} {'csw':>8}")
        for w in ws:
            r = runs[w]
            ps = r.get("per_state", {})
            rel = r.get("cpu_us_per_state", 0) / base["cpu_us_per_state"] if base.get("cpu_us_per_state") and r.get("cpu_us_per_state") else 0
            eff = base["wall_s"] / (r["wall_s"] * int(w) / int(ws[0])) if r.get("wall_s") else 0
            lines.append(f"  {w:>4} {r.get('wall_s', 0):8.2f} {r.get('cpu_s', 0):8.2f} {r.get('sys_s', 0):7.2f} {r.get('cpu_us_per_state', 0):9.3f} "
                         f"{rel:6.2f} {eff:5.0%} {r.get('ipc') or 0:5.2f} {r.get('ghz') or 0:5.2f} {ps.get('instructions', 0):9.0f} "
                         f"{ps.get('cycles', 0):8.0f} {ps.get('cache-misses', 0):8.1f} {(r.get('perf') or {}).get('context-switches') or 0:8.0f}")
    return lines
