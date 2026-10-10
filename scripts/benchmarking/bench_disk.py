#!/usr/bin/env python3
"""State spaces beyond memory (bench.py disk): tlx and TLC with far less
memory than a model needs (docs/scale.md).

Per model a plan of runs (--plan, comma-separated; `@a+b` restricts a tlx
item to binaries a and b):

  mem        each tlx binary in memory: its peak resident memory P is the
             reference for the fractions below (the first binary's)
  max:F      tlx -maxmem F*P (the page cache is not limited)
  cg:F       a machine with F*P of memory: tlx in a control group whose
             memory.max is F*P (page cache included, no swap), with its
             default budget (it must find the limit itself)
  cgmax:F    as cg:F, with -maxmem F*P/2 given explicitly
  tlc:F      TLC with -Xmx F*P/2 and -XX:MaxDirectMemorySize F*P/2
  tlccg:F    TLC in a control group of F*P with -Xmx F*P/2 and direct
             memory F*P/4 (TLC's disk fingerprint set and disk queue)
  tlcmem     TLC in memory (heap and direct memory 40% of RAM each)

Every run is sampled once a second: resident memory, bytes read and written
by the process (Linux /proc/<pid>/io: storage I/O, page-cache misses of
mapped files included), the control group's memory (page cache included),
the bytes in its spill directory, and the distinct states of the progress
reports (every 10 s: tlx -progress, TLC -Dtlc2.TLC.progressInterval=10).
tlx runs also record -stats json. Counts must equal the reference run's.
Each sampled run owns its child, its sampler and its reader: at the deadline,
or when any of them fails, the child is killed and reaped, the sampler stopped
and the failure reported with the run instead of being dropped.
Control groups need cgroup v2 and passwordless sudo (cloud instances).
Results go to bench/history/disk-<commit>.json (resumable: runs already
there are skipped unless --force), then the report (also: bench_disk.py
FILE).

CPU-heavy: run on a cloud instance (cloud_verify.py --bench "disk ...").
"""

if __package__ in (None, ""):
    import sys
    from pathlib import Path
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))


import tempdirs

from benchmarking import bench_budget

import dataclasses
import json
import os
import re
import shutil
import signal
import shlex
import subprocess
import sys
import tempfile
import threading
import time

from benchmarking import measurement
from tlxlib import ROOT

PROGRESS = re.compile(r"Progress\((\d+)\) at .*?: ([\d,]+) states generated.*?, ([\d,]+) distinct states found")
CGROOT = "/sys/fs/cgroup"

# (Bounded cleanup: a reader whose pipe a killed descendant still holds, a
# sample stuck in a large spill directory, or a signal that did not land must
# not hold the run open.)
READER_JOIN_S = 5
SAMPLER_JOIN_S = 5
CHILD_EXIT_GRACE_S = 10
KILL_GRACE_S = 10


def add_args(p):
    p.add_argument("--models", required=True, help="comma-separated ids (bench/<name> = bench/scaling/<name>.cfg)")
    p.add_argument("--bin", action="append", default=[], help="NAME=PATH of a tlx binary (repeatable; default main=target/release/tlx)")
    p.add_argument("--plan", default="mem,max:0.25,max:0.1,cg:0.25,tlccg:0.25", help="runs per model (see the module doc)")
    p.add_argument("--workers", type=int, default=0, help="tlx and TLC -workers (default: the number of CPUs)")
    p.add_argument("--ref-mib", action="append", default=[], help="ID=MIB: the in-memory peak of model ID when the plan has no mem run")
    p.add_argument("--args", default="-compile", help="tlx arguments besides -workers/-maxmem (default -compile)")
    p.add_argument("--env", action="append", default=[], help="NAME:K=V: environment of binary NAME's runs (repeatable)")
    p.add_argument("--timeout", type=int, default=1200)
    p.add_argument("--tlc-timeout", type=int, default=0, help="TLC timeout (default --timeout); a timed-out run reports its rate")
    p.add_argument("--force", action="store_true")
    p.add_argument("--out", default="")


class Sampler:
    """Samples a process (or the process in a control group) once a second:
    [t, rss MiB, read bytes, written bytes, spill directory bytes, distinct
    states of the last progress report, control group MiB].

    A sample that fails is kept in `problems` and stops the loop: a run whose
    memory or I/O was only partly observed is not a complete sample, and the
    caller has to hear about it (HARNESS-09)."""

    def __init__(self, pid, metadir, cgroup=None):
        self.pid, self.metadir, self.cgroup = pid, metadir, cgroup
        self.samples, self.distinct, self.problems = [], None, []
        self.peak_disk, self.peak_cg, self.io = 0, 0, {}
        self.stop = threading.Event()
        self.t0 = time.time()
        self.thread = threading.Thread(target=self.loop, daemon=True, name="bench-sampler")
        self.thread.start()

    def loop(self):
        while not self.stop.wait(1.0):
            try:
                self.sample()
            except Exception as error:  # (Named below; a dead sampler is not a short run.)
                self.problems.append(f"sampler: {type(error).__name__}: {error}")
                return

    def target(self):
        """The measured process: in a control group the one it holds (the
        child is sudo, which waits for it)."""
        if self.cgroup:
            try:
                with open(os.path.join(self.cgroup, "cgroup.procs")) as stream:
                    pids = stream.read().split()
                return int(pids[-1]) if pids else None
            except (OSError, ValueError):
                return None
        return self.pid

    def sample(self):
        pid = self.target()
        rss = proc_rss_mib(pid) if pid else None
        io = proc_io(pid) if pid else {}
        if io:
            self.io = io
        disk = dir_bytes(self.metadir)
        self.peak_disk = max(self.peak_disk, disk)
        cg = cg_read(self.cgroup, "memory.current") if self.cgroup else None
        if cg:
            self.peak_cg = max(self.peak_cg, cg)
        if rss is None:
            return
        self.samples.append([round(time.time() - self.t0, 1), round(rss, 1), io.get("read_bytes"), io.get("write_bytes"), disk,
                             self.distinct, None if cg is None else round(cg / (1 << 20), 1)])

    def line(self, text):
        if m := PROGRESS.search(text):
            self.distinct = int(m.group(3).replace(",", ""))

    def finish(self):
        """Stop the sampler and return everything that failed while observing:
        the join is bounded, so an unfinished sample cannot hang the run."""
        self.stop.set()
        self.thread.join(SAMPLER_JOIN_S)
        if self.thread.is_alive():
            self.problems.append("sampler: did not stop")
        return self.problems


def proc_rss_mib(pid):
    try:
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                if line.startswith("VmRSS:"):
                    return int(line.split()[1]) / 1024
    except OSError:
        pass
    if sys.platform == "darwin":
        try:
            out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
            return int(out) / 1024 if out else None
        except (OSError, ValueError):
            return None
    return None


def proc_io(pid):
    """Linux: /proc/<pid>/io (read_bytes/write_bytes: storage; rchar/wchar: system calls)."""
    try:
        with open(f"/proc/{pid}/io") as f:
            return {k: int(v) for k, v in (line.split(": ") for line in f)}
    except (OSError, ValueError):
        return {}


def dir_bytes(d):
    """Allocated bytes of the files under `d` (holes punched do not count)."""
    total = 0
    for root, _, files in os.walk(d):
        for name in files:
            try:
                total += os.lstat(os.path.join(root, name)).st_blocks * 512
            except OSError:
                pass
    return total


def cg_read(cg, name):
    try:
        with open(os.path.join(cg, name)) as stream:
            return int(stream.read().split()[0])
    except (OSError, ValueError, IndexError):
        return None


class Cgroup:
    """A control group with `mib` MiB of memory (page cache included) and no
    swap, made with sudo; `wrap` runs a command in it as the current user."""

    def __init__(self, mib):
        self.path = os.path.join(CGROOT, f"tlx-bench-{os.getpid()}-{int(time.time() * 1000) % 10**9}")
        self.mib = mib

    def __enter__(self):
        sudo = ["sudo", "-n"]
        try:
            subprocess.run(sudo + ["sh", "-c", "grep -qw memory \"$0/cgroup.subtree_control\" || echo +memory > \"$0/cgroup.subtree_control\"", CGROOT],
                           check=True)
            subprocess.run(sudo + ["mkdir", self.path], check=True)
            subprocess.run(sudo + ["sh", "-c", f"echo {self.mib}M > \"$0/memory.max\"; [ ! -e \"$0/memory.swap.max\" ] || echo 0 > \"$0/memory.swap.max\"",
                                   self.path], check=True)
        except subprocess.SubprocessError:
            # (A half-made control group is released here: `with` never calls
            # __exit__ when __enter__ raises.)
            self.__exit__(None, None, None)
            raise
        return self

    def wrap(self, cmd, env):
        # (sudo resets the environment: pass it through env(1).)
        script = 'cg="$0"; uid="$1"; gid="$2"; shift 2; echo $$ > "$cg/cgroup.procs" && exec setpriv --reuid="$uid" --regid="$gid" --init-groups -- env "$@"'
        return ["sudo", "-n", "sh", "-c", script, self.path, str(os.getuid()), str(os.getgid()),
                *[f"{k}={v}" for k, v in env.items() if "\n" not in v], *cmd]

    def events(self):
        out = {}
        try:
            with open(os.path.join(self.path, "memory.events")) as stream:
                for line in stream:
                    k, v = line.split()
                    out[k] = int(v)
        except OSError:
            pass
        return out

    def __exit__(self, *exc):
        for _ in range(50):
            if subprocess.run(["sudo", "-n", "rmdir", self.path], capture_output=True).returncode == 0:
                return
            time.sleep(0.2)
        # (Not raised: the measurements of this run are still good, but a
        # control group left in /sys/fs/cgroup has to be visible.)
        print(f"  warning: control group {self.path} not removed", flush=True)


@dataclasses.dataclass
class SampledRun:
    """One sampled child run: the child's own result and what was observed.

    `rusage` is this child's own wait4 usage, never a process-wide delta; it
    stays None when another waiter reaped the child first. `problems` names
    every observation that failed, so a caller never reports a partially
    observed run as a complete one (HARNESS-09)."""

    output: str
    returncode: int
    wall: float
    rusage: object
    sampler: Sampler
    timed_out: bool
    problems: list


def run_sampled(cmd, timeout, cwd=None, env=None, metadir=None, pass_fds=(), cgroup=None):
    """Run `cmd` (in `cgroup` if given) with sampling: a `SampledRun`.

    This call owns the child, its sampler and its reader: however it returns
    or raises (a launch failure, a sampler or reader failure, the deadline, a
    wait4 that lost the child), the child is killed and reaped, the sampler
    stopped, the reader joined and the pipe closed before it continues. A
    failure to observe is kept in `problems` instead of being reported as a
    complete measurement."""
    env = env if env is not None else dict(os.environ)
    if cgroup:
        cmd = cgroup.wrap(cmd, env)
    t0 = time.time()
    p = subprocess.Popen(cmd, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1,
                         errors="replace", pass_fds=pass_fds)
    sampler = reader = None
    lines, problems = [], []
    # (Set before we close the pipe ourselves: the reader's own ValueError is
    # then our deliberate unblocking, not an observation failure.)
    unblocked = threading.Event()
    timed_out, ru = False, None
    try:
        sampler = Sampler(p.pid, metadir or "/nonexistent", cgroup.path if cgroup else None)

        def read():
            try:
                for line in p.stdout:
                    lines.append(line)
                    sampler.line(line)
            except Exception as error:
                if not unblocked.is_set():
                    problems.append(f"reader: {type(error).__name__}: {error}")
        reader = threading.Thread(target=read, daemon=True, name="bench-reader")
        reader.start()
        while True:
            # (The usage of this child and its waited-for descendants: wait4.)
            try:
                pid, status, ru = os.wait4(p.pid, os.WNOHANG)
            except ChildProcessError as error:
                # (Another waiter reaped this child: its own usage is gone, and
                # no process-wide child delta may stand in for it.)
                problems.append(f"wait4: {error}")
                break
            if pid:
                # (Only this call carries the status and the usage: the
                # WNOHANG polls that found it running must not be read as one.)
                p.returncode = os.waitstatus_to_exitcode(status)
                break
            elapsed = time.time() - t0
            if elapsed >= timeout and not timed_out:
                timed_out = True
                # (In a control group the measured process is sudo's child. Both
                # are ours: this pid and the one our own cgroup holds.)
                target = sampler.target()
                for t in sorted({target, p.pid} - {None}):
                    if refusal := kill_pid(t, cgroup is not None and t != p.pid):
                        problems.append(refusal)
            elif timed_out and elapsed >= timeout + KILL_GRACE_S:
                # (The signal did not land - refused, or the child is stuck.
                # Stop waiting on it here; the finally kills and reaps it.)
                break
            time.sleep(0.2)
    finally:
        wall = time.time() - t0
        if p.returncode is None:
            # (An error path left the child running: kill only it and reap it
            # here, or it keeps writing to the spill directory the caller is
            # about to remove.)
            try:
                p.kill()
            except OSError:
                pass
            try:
                p.wait(timeout=CHILD_EXIT_GRACE_S)
            except (subprocess.TimeoutExpired, OSError) as error:
                problems.append(f"child {p.pid}: exit not collected here ({error})")
        if reader is not None:
            reader.join(READER_JOIN_S)
            # (Only a descendant still holding the pipe keeps it open.)
            if reader.is_alive():
                unblocked.set()
        if p.stdout is not None:
            p.stdout.close()
        if reader is not None:
            reader.join(READER_JOIN_S)
            if reader.is_alive():
                problems.append("reader: still reading after its pipe was closed")
        problems.extend(sampler.finish() if sampler else ["sampler: not started"])
    return SampledRun("".join(lines), p.returncode, wall, ru, sampler, timed_out, problems)


def kill_pid(pid, as_root):
    """Signal exactly this pid (as root only for the control group's own
    child) and return why it was refused, or None when it was sent."""
    if as_root:
        done = subprocess.run(["sudo", "-n", "kill", "-KILL", str(pid)], capture_output=True)
        if done.returncode != 0:
            why = done.stderr.decode(errors="replace").strip() or f"exit {done.returncode}"
            return f"kill {pid} refused: {why}"
        return None
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    except OSError as error:
        return f"kill {pid} failed: {error}"
    return None


def finish_record(r, run, timeout, cgroup=None):
    """Add a sampled run's own accounting to its parsed result.

    CPU and peak RSS come from that child's wait4 usage; when the observation
    was lost they are reported as unknown and `problems` says why (HARNESS-09)."""
    ru = run.rusage
    if ru is not None:
        r["cpu_s"] = measurement.cpu_seconds(ru, decimals=2)
        r["maxrss_mib"] = measurement.rss_mib(ru)
    r["io"] = {k: run.sampler.io.get(k) for k in ("read_bytes", "write_bytes", "rchar", "wchar") if k in run.sampler.io}
    r["disk_peak_bytes"] = run.sampler.peak_disk
    r["samples"] = run.sampler.samples
    if cgroup:
        r["cgroup_mib"] = cgroup.mib
        r["cgroup_peak_mib"] = round((cg_read(cgroup.path, "memory.peak") or run.sampler.peak_cg) / (1 << 20), 1)
        r["cgroup_events"] = cgroup.events()
        if r["cgroup_events"].get("oom_kill"):
            r["result"] = "oom-killed"
    if run.problems:
        r["problems"] = list(run.problems)
        if r.get("result") == "success":
            r["result"], r["error"] = "bench-error", "; ".join(run.problems)
    if run.timed_out:
        # (The rate up to the timeout comes from the last progress sample.)
        r.update({"result": "timeout", "wall_s": timeout})
    return r


@bench_budget.model_pass
def run_tlx(m, binary, args, timeout, env, cg_mib=None):
    from tlc import parse
    # (The state directory and the -stats file need a directory of their own
    # even for a model that writes nothing, and the control-group member reads
    # them through sudo: the staged root is made world-readable.)
    with measurement.staged_run(m, "tlx-disk-", workspace=True, mode=0o755) as (m, cwd, tmp):
        md = os.path.join(tmp, "states")
        stats = os.path.join(tmp, "stats.json")
        cmd = measurement.check_argv(measurement.binary_path(binary), m, args,
                                     extra=["-progress", "-metadir", md, "-stats", f"json={stats}"])
        e = measurement.measured_env(env)
        if cg_mib:
            with Cgroup(cg_mib) as cg:
                run = run_sampled(cmd, timeout, cwd=cwd, env=e, metadir=md, cgroup=cg)
                r = finish_record(parse(run.output, run.wall, run.returncode), run, timeout, cg)
        else:
            run = run_sampled(cmd, timeout, cwd=cwd, env=e, metadir=md)
            r = finish_record(parse(run.output, run.wall, run.returncode), run, timeout)
        r["cmd"] = " ".join(cmd[2:])
        r["tail"] = run.output.splitlines()[-6:]
        try:
            with open(stats) as stream:
                r["stats"] = json.load(stream)
            r["stats"].pop("levels", None)
        except (OSError, ValueError):
            pass
        return r


def run_tlc(m, workers, timeout, heap_mib, direct_mib, cg_mib=None):
    from benchmarking.bench_records import cached_tlc
    keyed = {**m, "measurement_constraints": {"heap_mib": heap_mib, "direct_mib": direct_mib, "cgroup_mib": cg_mib}}
    return cached_tlc(keyed, workers, lambda: _collect_tlc(m, workers, timeout, heap_mib, direct_mib, cg_mib))


@bench_budget.model_pass
def _collect_tlc(m, workers, timeout, heap_mib, direct_mib, cg_mib=None):
    from tlc import parse, tlc_cmd
    from tlxlib import check_tlc_model, tlc_slot
    tmp = tempdirs.mkdtemp(prefix="tlc-disk-")
    os.chmod(tmp, 0o755)
    try:
        md = os.path.join(tmp, "states")
        # (Progress reports every 10 s, as tlx -progress.)
        jvm = [f"-Xmx{heap_mib}m", f"-XX:MaxDirectMemorySize={direct_mib}m", "-Dtlc2.TLC.asMilliSeconds=true",
               "-Dtlc2.TLC.progressInterval=10"]
        cmd = tlc_cmd(m["tla"], ["-config", m["cfg"], "-workers", str(workers)], m.get("features") or (), jvm_extra=jvm, state_dir=md)
        cwd = os.path.dirname(os.path.abspath(m["tla"]))
        check_tlc_model(cmd, cwd)
        with tlc_slot() as slot:
            fds = (slot.fileno(),) if slot is not None else ()
            if cg_mib:
                with Cgroup(cg_mib) as cg:
                    run = run_sampled(cmd, timeout, cwd=cwd, metadir=md, pass_fds=fds, cgroup=cg)
                    r = finish_record(parse(run.output, run.wall, run.returncode), run, timeout, cg)
            else:
                run = run_sampled(cmd, timeout, cwd=cwd, metadir=md, pass_fds=fds)
                r = finish_record(parse(run.output, run.wall, run.returncode), run, timeout)
        r["cmd"] = " ".join(jvm[:2] + ["-workers", str(workers)])
        if mm := re.search(r"workers? on \d+ cores with (.*?) \[pid: \d+\] \((.*)\)", run.output):
            r["tlc_memory"] = f"{mm.group(1)}; {mm.group(2)}"
        if mm := re.search(r"Finished in (\d+)ms at", run.output):
            r["finished_s"] = int(mm.group(1)) / 1000
        r["tail"] = run.output.splitlines()[-6:]
        return r
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def counts(r):
    return tuple(r.get(k) for k in ("distinct", "generated", "depth"))


def gib(b):
    return "-" if b is None else f"{b / (1 << 30):.2f}"


def number(value, spec, unit=""):
    """A measured value, or "-" when its observation was lost (never a zero
    that would read as a measurement)."""
    return "-" if value is None else f"{value:{spec}}{unit}"


def describe(r):
    io = r.get("io") or {}
    cg = f" cgroup peak {r['cgroup_peak_mib']:.0f}M" if r.get("cgroup_peak_mib") else ""
    return (f"{r.get('result')} {r.get('distinct')} wall {r.get('wall_s', 0):.1f}s cpu {number(r.get('cpu_s'), '.0f', 's')} "
            f"rss {number(r.get('maxrss_mib'), '.0f', 'M')}{cg} write {gib(io.get('write_bytes'))}G read {gib(io.get('read_bytes'))}G "
            f"disk {gib(r.get('disk_peak_bytes'))}G")


def parse_plan(plan, bins):
    """[(kind, fraction or None, [bin names])]"""
    out = []
    for item in [x.strip() for x in plan.split(",") if x.strip()]:
        item, _, only = item.partition("@")
        kind, _, f = item.partition(":")
        if kind not in ("mem", "max", "cg", "cgmax", "tlc", "tlccg", "tlcmem"):
            sys.exit(f"--plan: unknown item {item}")
        names = only.split("+") if only else [n for n, _ in bins]
        if bad := set(names) - {n for n, _ in bins}:
            sys.exit(f"--plan {item}: unknown binaries {sorted(bad)}")
        out.append((kind, float(f) if f else None, names))
    return out


def run(a):
    from bench import build_release
    release = os.path.join(ROOT, "target", "release", "tlx")
    bins = [b.split("=", 1) for b in a.bin] or [["main", release]]
    if any(os.path.abspath(os.path.join(ROOT, p)) == release for _, p in bins):
        # (This tree's release binary: built first, run from a private copy.)
        with build_release("tlx-disk-") as binary:
            run_bins(a, [[n, binary if os.path.abspath(os.path.join(ROOT, p)) == release else p] for n, p in bins])
    else:
        run_bins(a, bins)


def run_bins(a, bins):
    from benchmarking.models import scale_models
    from benchmarking.machine import machine_desc, git_commit, mem_bytes
    from bench import HISTORY
    w = a.workers or os.cpu_count() or 1
    paths = {n: os.path.join(ROOT, p) for n, p in bins}
    envs = {}
    for e in a.env:
        name, kv = e.split(":", 1)
        k, v = kv.split("=", 1)
        envs.setdefault(name, {})[k] = v
    plan = parse_plan(a.plan, bins)
    refs = {k: int(v) for k, v in (x.split("=", 1) for x in a.ref_mib)}
    ms = scale_models([k.strip() for k in a.models.split(",") if k.strip()])
    out = a.out or os.path.join(HISTORY, f"disk-{git_commit()}.json")
    h = {}
    if os.path.exists(out):
        with open(out) as stream:
            h = json.load(stream)
    h.update({"kind": "disk", "date": h.get("date") or time.strftime("%Y-%m-%d %H:%M:%S"), "machine": machine_desc(),
              "commit": git_commit(), "mem_gib": round(mem_bytes() / (1 << 30), 1), "workers": w, "plan": a.plan,
              "bins": {n: os.path.relpath(p, ROOT) for n, p in bins}, "args": a.args})
    models = h.setdefault("models", {})

    def save():
        bench_budget.record(h)
        h["date_end"] = time.strftime("%Y-%m-%d %H:%M:%S")
        os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
        with open(out, "w") as stream:
            json.dump(h, stream, indent=1, sort_keys=True)
    print(f"disk scaling on {h['machine']}: {len(ms)} models, {w} workers, plan {a.plan}, bins {[n for n, _ in bins]} -> "
          f"{os.path.relpath(out, ROOT)}", flush=True)
    base = a.args.split() + ["-workers", str(w)]
    tlc_timeout = a.tlc_timeout or a.timeout
    for k, m in ms.items():
        rec = models.setdefault(k, {})
        runs = rec.setdefault("runs", {})

        def do(label, f):
            if label in runs and not a.force and not label.startswith("TLC"):
                return runs[label]
            try:
                r = f()
            except (OSError, subprocess.SubprocessError) as e:
                # (E.g. no control groups here: the other runs go on.)
                r = {"result": "bench-error", "error": str(e)}
            runs[label] = r
            print(f"  {k:<50} {label:<16} {describe(r)}", flush=True)
            save()
            return r
        for kind, fr, names in plan:
            if kind == "mem":
                for n in names:
                    do(f"{n} mem", lambda: run_tlx(m, paths[n], base, a.timeout, envs.get(n)))
                continue
            if kind == "tlcmem":
                full = int(mem_bytes() * 0.4) >> 20
                do("TLC mem", lambda: run_tlc(m, w, tlc_timeout, full, full))
                continue
            ref = refs.get(k) or rec.get("ref_mib") or runs.get(f"{bins[0][0]} mem", {}).get("maxrss_mib")
            if not ref:
                print(f"  {k}: no reference peak for {kind}:{fr:g}, skipped", flush=True)
                continue
            rec["ref_mib"] = ref
            mib = max(64, int(ref * fr))
            if kind == "max":
                for n in names:
                    do(f"{n} max {fr:g}", lambda: run_tlx(m, paths[n], base + ["-maxmem", f"{mib}M"], a.timeout, envs.get(n)))
            elif kind == "cg":
                for n in names:
                    do(f"{n} cg {fr:g}", lambda: run_tlx(m, paths[n], base, a.timeout, envs.get(n), cg_mib=mib))
            elif kind == "cgmax":
                for n in names:
                    do(f"{n} cgmax {fr:g}", lambda: run_tlx(m, paths[n], base + ["-maxmem", f"{mib // 2}M"], a.timeout, envs.get(n), cg_mib=mib))
            elif kind == "tlc":
                do(f"TLC {fr:g}", lambda: run_tlc(m, w, tlc_timeout, max(256, mib // 2), max(256, mib // 2)))
            elif kind == "tlccg":
                do(f"TLC cg {fr:g}", lambda: run_tlc(m, w, tlc_timeout, max(256, mib // 2), max(128, mib // 4), cg_mib=mib))
    from benchmarking.bench_records import ingest
    ingest(out)
    print(f"recorded in {os.path.relpath(out, ROOT)}\n")
    print("\n".join(report(h)))


def rate(r):
    """Distinct states per second: over the run, or up to the last progress
    report of a run that timed out."""
    if r.get("result") == "success" and r.get("distinct") and r.get("wall_s"):
        return r["distinct"] / r["wall_s"]
    last = next((s for s in reversed(r.get("samples") or []) if s[5]), None)
    return last[5] / last[0] if last and last[0] else None


def row_order(label):
    """In-memory runs first, then by limit (largest first), tlx before TLC."""
    last = label.split()[-1]
    f = float(last) if last.replace(".", "", 1).isdigit() else float("inf")
    return (-f, label.startswith("TLC"), label)


def report(h):
    out = [f"{h.get('machine')}, {h.get('mem_gib')} GiB, {h.get('workers')} workers, commit {h.get('commit')}, plan {h.get('plan')}", ""]
    for k, rec in h["models"].items():
        runs = rec.get("runs", {})
        ref = next((counts(r) for lbl, r in runs.items() if lbl.endswith(" mem") and r.get("result") == "success"), None)
        out += [f"### {k} (in-memory peak {rec.get('ref_mib') or 0:.0f} MiB)", "",
                "| run | memory | result | wall s | distinct/s | peak RSS MiB | anon peak MiB | cgroup peak MiB | written GiB | read GiB | "
                "disk peak GiB | fp pages on disk | counts |",
                "|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|:-:|"]
        for lbl, r in sorted(runs.items(), key=lambda x: row_order(x[0])):
            io = r.get("io") or {}
            st = r.get("stats") or {}
            budget = (st.get("memory") or {}).get("budget (-maxmem)")
            # (tlx's own peak: anonymous resident memory on Linux, without the mapped run files.)
            anon = (st.get("memory") or {}).get("peak RSS")
            lookups = (st.get("phases") or {}).get("fpset disk lookups (pages read)")
            mem = []
            if isinstance(budget, (int, float)):
                mem.append(f"budget {budget / (1 << 20):.0f}M")
            if lbl.startswith("TLC"):
                mem.append(r.get("cmd", "").split(" -workers")[0])
            if r.get("cgroup_mib"):
                mem.append(f"cgroup {r['cgroup_mib']}M")
            ok = "-" if "-tlc" not in shlex.split(h.get("args", "")) or r.get("result") != "success" or not ref else ("✓" if counts(r)[:2] == ref[:2] else "✗")
            rt = rate(r)
            cg = r.get("cgroup_peak_mib")
            out.append(f"| {lbl} | {', '.join(mem) or '-'} | {r.get('result')} | {r.get('wall_s', 0):.1f} | {'-' if rt is None else f'{rt:,.0f}'} | "
                       f"{number(r.get('maxrss_mib'), '.0f')} | {'-' if not isinstance(anon, (int, float)) else f'{anon / (1 << 20):.0f}'} | "
                       f"{'-' if cg is None else f'{cg:.0f}'} | {gib(io.get('write_bytes'))} | "
                       f"{gib(io.get('read_bytes'))} | {gib(r.get('disk_peak_bytes'))} | "
                       f"{'-' if not isinstance(lookups, (int, float)) else f'{lookups / 1e6:.1f}M'} | {ok} |")
        out.append("")
    return out


if __name__ == "__main__":
    with open(sys.argv[1]) as stream:
        print("\n".join(report(json.load(stream))))
