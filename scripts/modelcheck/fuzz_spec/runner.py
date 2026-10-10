"""Bounded state-spec execution, checkpoint recovery and mode orchestration."""
import hashlib
import os
import shutil
import tempfile
import threading
import time
import tempdirs
from modelcheck import fuzz_expr as fx
from modelcheck.results import must
from tlc import tlc_cmd
from tlxlib import IR_REPORT, NATIVE_REPORT, parity_args, parity_env, run_proc, tlx_env
from .constants import ABORT_EXIT, FILTER_TIMEOUT, TLC_TIMEOUT, SAFETY
from .judgment import brief, heterogeneous_issue, judge, known_issue
from .render import features, render, write_spec
import tracecheck


def mode_variant(name, mode):
    """The concrete flags of `mode` for spec `name` (deterministic per spec)."""
    h = int(hashlib.sha1(name.encode()).hexdigest()[:8], 16)
    if mode == "interp":
        return [], {}, 1
    if mode == "compile":
        return ["-compile"], {"TLX_PRERUN_REUSE": "0"}, 1
    if mode == "paranoid":
        return ["-compile"], [{"TLX_PARANOID": "1"}, {"TLX_PARANOID": "1", "TLX_JIT": "1"},
                              {"TLX_PARANOID": "1", "TLX_PRERUN": "1"}][h % 3], 1
    if mode == "w4":
        return (["-compile"] if h % 2 == 0 else []), {}, 4
    if mode == "recover":
        flags, workers = [(["-compile"], 1), ([], 1), (["-compile"], 4)][(h // 3) % 3]
        return flags, {}, workers
    raise ValueError(mode)



def run_tlc(d, name, timeout=TLC_TIMEOUT, workers=1):
    """TLC's -tool output, exit code and wall time (None output: timeout)."""
    cmd = tlc_cmd(name + ".tla", ["-config", name + ".cfg", "-workers", str(workers), "-tool", "-fp", "0"], jvm_extra=["-Xmx1g"],
                  state_dir=tempdirs.mkdtemp(prefix="tlc-states-", dir=d))
    p = run_proc(cmd, timeout, cwd=d, raise_timeout=False)
    shutil.rmtree(cmd[cmd.index("-metadir") + 1], ignore_errors=True)
    if p.timed_out:
        return None, -1, p.wall
    return p.stdout + p.stderr, p.returncode, p.wall



def run_tlx(binary, d, name, mode, timeout, distinct=None, key=None):
    """tlx's result in `mode` (the `must` fields plus `out`; result
    "timeout" on a timeout); `key` selects the mode's variant (default:
    the spec's name)."""
    flags, env, workers = mode_variant(key or name, mode)
    flags = parity_args(binary, flags)
    env = {**parity_env(flags), **env}
    cmd = [binary, "check", name + ".tla", "-config", name + ".cfg", "-tool", "-workers", str(workers), *flags]
    if mode != "recover":
        return tlx_once(cmd, d, env, timeout)
    md = tempfile.mkdtemp(prefix="recover-", dir=d)
    try:
        every = max(1, (distinct or 3) // 3)
        t0 = time.time()
        r = tlx_once(cmd + ["-metadir", os.path.join(md, "md")], d,
                     {**env, "TLX_CHECKPOINT_STATES": str(every), "TLX_CHECKPOINT_ABORT": "1"}, timeout)
        if r.get("exit") != ABORT_EXIT:
            return r
        r = tlx_once(cmd + ["-recover", os.path.join(md, "md")], d, env, max(1, timeout - (time.time() - t0)))
        r["recovered"] = True
        return r
    finally:
        shutil.rmtree(md, ignore_errors=True)



def tlx_once(cmd, d, env, timeout):
    p = run_proc(cmd, timeout, cwd=d, env=tlx_env(classpath=True, extra=env), raise_timeout=False, grace=2)
    out = p.stdout + p.stderr
    if p.timed_out:
        return {"result": "timeout", "out": out[-3000:], "wall": p.wall}
    r = must(out, p.returncode)
    r["out"], r["wall"] = out, p.wall
    return r



def symmetry_ok(binary, d, s):
    """Whether tlx -checksymmetry finds no witness against the SYMMETRY."""
    p = run_proc([binary, "check", s.name + ".tla", "-config", s.name + ".cfg", "-checksymmetry",
                  "-symmetrystates", "3000", "-symmetrydepth", "8", *parity_args(binary, [])], 60, cwd=d, env=tlx_env(classpath=True), raise_timeout=False)
    return not p.timed_out and p.returncode == 0



class Runner:
    def __init__(self, a, binary, out_dir):
        self.a, self.binary, self.out_dir = a, binary, out_dir
        self.lock = threading.Lock()
        self.validated = {}
        self.stats = {}

    def bump(self, key, n=1):
        with self.lock:
            self.stats[key] = self.stats.get(key, 0) + n

    def compare(self, d, name, want, got, workers):
        """The same contract for generated specs and saved regressions."""
        diffs, note = judge(d, name, want, got, workers, self.validated)
        if diffs and workers > 1 and set(diffs) <= {"result", "violated"}:
            return self.several_workers(d, name, want, got, workers, diffs, note)
        return diffs, note

    def several_workers(self, d, name, want, got, workers, diffs, note):
        """Accept another same-level safety violation only after TLC validates
        its concrete behavior and the property it actually reports (D-3).
        Depth alone cannot establish that a counterexample is sound."""
        w = want.get("depth") if want.get("result") != "success" else None
        g = got.get("depth")
        if w is not None and g is not None and abs(w - g) <= 1 and got.get("result") in SAFETY:
            cfg = tracecheck.read(os.path.join(d, name + ".cfg"))
            states, loop = got["lasso"]
            names = [got["violated"]] if got.get("violated") else None
            verdict, why = tracecheck.check(d, name, cfg, got["result"], states, loop, names, cache=self.validated)
            if verdict == "valid":
                self.bump("valid violation at the same level (D-3)")
                return [], f"valid counterexample at the same BFS level (D-3; TLC depth {w}, tlx {g})"
            return diffs, note + f" (alternative counterexample {verdict}: {why})"
        return diffs, note + " (no same-level validated counterexample)"

    def note_native(self, mode, out):
        """Count what the compiled tier ran natively (its compile: lines)."""
        if m := NATIVE_REPORT.search(out):
            self.bump(f"native {mode} actions", int(m.group(1)))
            self.bump(f"native {mode} actions total", int(m.group(2)))
        if m := IR_REPORT.search(out):
            self.bump(f"IR {mode} nodes", int(m.group(1)))
            self.bump(f"IR {mode} interpreter nodes", int(m.group(3)))

    def check(self, s, d, modes, tlc_out=None):
        """Run spec `s` (written to `d`) through TLC and tlx in `modes`:
        {"status": "pass" | "fail" | "skip", "why", "tlc", "modes": {mode: {...}}}."""
        write_spec(d, s)
        res = {"name": s.name, "features": features(s), "modes": {}}
        if s.symmetry and not symmetry_ok(self.binary, d, s):
            s.symmetry = None
            s.features["symmetry_dropped"] = True
            write_spec(d, s)
            res["features"] = features(s)
            self.bump("symmetry dropped")
        elif s.symmetry:
            self.bump("symmetry kept")
        first = run_tlx(self.binary, d, s.name, "interp", FILTER_TIMEOUT)
        if first["result"] == "timeout":
            return {**res, "status": "skip", "why": "too big (interpreter)"}
        if tlc_out is None:
            out, rc, wall = run_tlc(d, s.name)
            if out is None:
                return {**res, "status": "skip", "why": "too big (TLC timeout)"}
            with open(os.path.join(d, "tlc.out"), "w") as f:
                f.write(out)
        else:
            out, rc, wall = tlc_out, None, 0
        want = must(out, rc)
        res["tlc"] = brief(want)
        if want["result"] in (None, "unknown"):
            return {**res, "status": "skip", "why": "TLC: no result: " + out[-300:]}
        if "Semantic errors" in out or "*** Errors:" in out or "Parse Error" in out:
            return {**res, "status": "skip", "why": "invalid spec (generator): " + out[-500:]}
        fails = {}
        for mode in modes:
            got = first if mode == "interp" else run_tlx(self.binary, d, s.name, mode, max(60, 20 * first["wall"]),
                                                         want.get("distinct"), key=s.variant)
            workers = mode_variant(s.variant or s.name, mode)[2]
            diffs, note = self.compare(d, s.name, want, got, workers)
            res["modes"][mode] = {"tlx": brief(got), "diffs": diffs, "note": note}
            if got.get("recovered"):
                res["modes"][mode]["recovered"] = True
                self.bump("recovered from a checkpoint")
            self.note_native(mode, got.get("out", ""))
            if diffs and (k := known_issue(out, got.get("out", ""), mode, "\n".join(render(s)), diffs)):
                res["modes"][mode]["known"] = k
                self.bump(f"known {k[0]}")
            if (diffs and not res["modes"][mode].get("known") and (op := s.features.get("heterogeneous"))
                    and (issue := heterogeneous_issue(want, got, mode, op, diffs, out, got.get("out", ""),
                                                     s.features.get("heterogeneous_kinds", ())))):
                res["modes"][mode]["known"] = (issue, fx.HETERO_OPEN[issue])
                self.bump(f"known {issue}")
            if diffs:
                fails[mode] = diffs
                with open(os.path.join(d, f"tlx-{mode}.out"), "w") as f:
                    f.write(got.get("out", ""))
        res["status"] = ("fail" if any(not r.get("known") for r in res["modes"].values() if r["diffs"]) else
                         "xfail" if fails else "pass")
        res["result"] = want["result"]
        return res
