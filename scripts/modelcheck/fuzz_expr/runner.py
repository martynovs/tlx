"""Run TLC and tlx for one generated spec.

``Runner`` builds work directories, invokes the engines with an explicit
execution mode, counts the compiled tier's native coverage and attributes a
state-level failure to one expression.
"""
import os
import re
import shutil
import subprocess
import threading

from contract import compare as compare_contract
from tlc import tlc_cmd
from tlxlib import IR_REPORT, NATIVE_REPORT, parity_args, run_proc, tlx_env

from .constants import TLC_SEM, TLX
from .render import PRINT_RE, const_cfg, const_spec, norm_error, state_spec, unescape

# ---------------------------------------------------------------------------
# Running TLC and tlx
# ---------------------------------------------------------------------------


class Runner:
    def __init__(self, args, work, binary=TLX):
        self.args = args
        self.work = work
        self.binary = binary
        self.lock = threading.Lock()
        self.counter = 0
        self.stats = {"tlc_runs": 0, "tlx_runs": 0}

    def tmpdir(self, tag):
        with self.lock:
            self.counter += 1
            d = os.path.join(self.work, f"{tag}-{self.counter}")
        os.makedirs(d, exist_ok=True)
        return d

    def run(self, kind, d, name, cfg, flags=(), env=None, timeout=120):
        if kind == "tlc":
            cmd = tlc_cmd(name + ".tla", ["-workers", "1", "-config", cfg], jvm_extra=["-Xmx1g"], state_dir=os.path.join(d, "states"))
            self.stats["tlc_runs"] += 1
            sem = TLC_SEM
        else:
            cmd = [self.binary, "check", name + ".tla", "-config", cfg, "-workers", "1", *parity_args(self.binary, flags)]
            self.stats["tlx_runs"] += 1
            sem = None
        e = tlx_env(drop=("TLX_PARANOID", "TLX_INJECT_MISMATCH", "TLX_NO_NATIVE", "TLX_PRERUN", "TLX_NATIVE_SYNC"),
                    classpath=True, extra=env)
        try:
            if sem:
                sem.acquire()
            p = run_proc(cmd, timeout, cwd=d, env=e)
            return p.stdout + p.stderr, p.returncode
        except subprocess.TimeoutExpired as ex:
            out = (ex.stdout or b"").decode() if isinstance(ex.stdout, bytes) else (ex.stdout or "")
            return out + "\n@@TIMEOUT@@", -1
        finally:
            if sem:
                sem.release()

    def note_coverage(self, mode, out):
        """Count what the compiled tier ran natively (the action and invariant
        totals of the compile log)."""
        key = {"compile": "native", "compile-ir": "native"}.get(mode)
        if not key:
            return
        with self.lock:
            st = self.stats.setdefault(mode, {})
            if m := NATIVE_REPORT.search(out):
                for k, v in zip(("native_actions", "actions", "native_invs", "invs"), m.groups()):
                    st[k] = st.get(k, 0) + int(v)
            if m := IR_REPORT.search(out):
                for k, v in zip(("ir_nodes", "ir_boxed", "ir_interp"), m.groups()):
                    st[k] = st.get(k, 0) + int(v)
            if m := re.search(r"IR: (\d+)/(\d+) actions lowered", out):
                st["lowered"] = st.get("lowered", 0) + int(m.group(1))
                st["actions"] = st.get("actions", 0) + int(m.group(2))

    def eval_const(self, kind, exprs, pool_line, tag="c"):
        """Evaluate expressions at constant level: {i: ("ok", text) | ("err", msg)}."""
        d = self.tmpdir(f"{tag}-{kind}")
        src, _ = const_spec("Fuzz", exprs, pool_line)
        with open(os.path.join(d, "Fuzz.tla"), "w") as f:
            f.write(src)
        res = {}
        start = 0
        while start < len(exprs):
            with open(os.path.join(d, "Fuzz.cfg"), "w") as f:
                f.write(const_cfg(start))
            out, rc = self.run(kind, d, "Fuzz", "Fuzz.cfg")
            for m in PRINT_RE.finditer(out):
                i = int(m.group(1))
                if i >= start and i not in res:
                    res[i] = ("ok", re.sub(r"\s+", " ", unescape(m.group(2))))
            nxt = start
            while nxt in res:
                nxt += 1
            if nxt >= len(exprs):
                break
            if "@@TIMEOUT@@" in out:
                res[nxt] = ("timeout", "")
            elif "Parsing or semantic analysis failed" in out or "*** Errors:" in out:
                raise SystemExit(f"{kind}: the generated spec does not parse ({d}):\n{out[-2000:]}")
            else:
                msg = norm_error(out)
                if msg is None or rc < 0:
                    res[nxt] = ("infrastructure", "missing output or abnormal exit: " + out[-300:], rc)
                else:
                    res[nxt] = ("err", msg, rc)
            start = nxt + 1
        if not self.args.keep:
            shutil.rmtree(d, ignore_errors=True)
        return res

    # -- state level ------------------------------------------------------
    def eval_state(self, mode, items, pool_line, tag="s"):
        """items: list of (expr Node, ctx, expected text or None (error), lift prob seed).
        Returns {index: None (agrees) | problem string}."""
        res = {}
        self._state_rec(mode, list(range(len(items))), items, pool_line, tag, res)
        return res

    def _state_rec(self, mode, idx, items, pool_line, tag, res):
        if not idx:
            return
        out, rc, lines = self.state_run(mode, [items[i] for i in idx], pool_line, tag)
        expect_err = len(idx) == 1 and items[idx[0]][2] is None
        if rc == 0 and "Model checking completed. No error has been found." in out:
            for i in idx:
                res[i] = "expected an error, got none" if items[i][2] is None else None
            return
        if expect_err:
            msg = state_error(out)
            iout, _, _ = self.state_run("interp", [items[idx[0]]], pool_line, tag)
            want = state_error(iout)
            if "TLX_PARANOID" in out:
                res[idx[0]] = "paranoid: " + paranoid_text(out)
            elif rc < 0:
                res[idx[0]] = "abnormal exit: " + out[-300:]
            elif msg is None:
                res[idx[0]] = "no error: " + out[-300:]
            elif compare_contract({"result": "error" if want is not None else "success"},
                                  {"result": "error"}).differs:
                res[idx[0]] = f"error-vs-value: {msg!r} vs interpreter {iout[-300:]!r}"
            else:
                res[idx[0]] = None
            return
        if len(idx) == 1 and mode != "interp" and rc >= 0 and rc != 101 and "TLX_PARANOID" not in out and "@@TIMEOUT@@" not in out:
            # Lifting a literal into a state variable adds fingerprinting.
            # A constant membership expression may succeed while the lifted
            # mixed set cannot be stored. Compare the same state program.
            if state_error(out) is not None:
                iout, irc, _ = self.state_run("interp", [items[idx[0]]], pool_line, tag)
                if irc >= 0 and irc != 101 and state_error(iout) is not None and "TLX_PARANOID" not in iout and "@@TIMEOUT@@" not in iout:
                    res[idx[0]] = None
                    return
        # Attribute the failure to one expression if possible.
        bad = None
        if m := re.search(r"Invariant Inv_(\d+) is violated", out):
            bad = int(m.group(1))
        else:
            for m in re.finditer(r"line (\d+), col \d+", out):
                ln = int(m.group(1))
                if ln in lines:
                    bad = lines[ln]
                    break
        if bad is not None and 0 <= bad < len(idx):
            if len(idx) == 1:
                res[idx[bad]] = describe_state_failure(out)
            else:
                self._state_rec(mode, [idx[bad]], items, pool_line, tag, res)
            rest = idx[:bad] + idx[bad + 1:]
            self._state_rec(mode, rest, items, pool_line, tag, res)
            return
        if len(idx) == 1:
            res[idx[0]] = describe_state_failure(out)
            return
        h = len(idx) // 2
        self._state_rec(mode, idx[:h], items, pool_line, tag, res)
        self._state_rec(mode, idx[h:], items, pool_line, tag, res)

    def state_run(self, mode, items, pool_line, tag):
        d = self.tmpdir(f"{tag}-{mode}")
        src, lines = state_spec("FuzzS", items, pool_line)
        with open(os.path.join(d, "FuzzS.tla"), "w") as f:
            f.write(src)
        invs = " ".join(f"Inv_{i}" for i in range(len(items)))
        with open(os.path.join(d, "FuzzS.cfg"), "w") as f:
            f.write(f"CONSTANTS m1 = m1 m2 = m2 m3 = m3\nINIT Init\nNEXT Next\nINVARIANT {invs}\n")
        if mode == "compile":
            flags, env = ["-compile", "-deadlock"], {"TLX_PARANOID": "1", "TLX_PRERUN": "1"}
        elif mode == "compile-ir":
            flags, env = ["-compile", "-deadlock"], {"TLX_PARANOID": "1", "TLX_PRERUN": "1", "TLX_NO_NATIVE": "1"}
        else:
            flags, env = ["-deadlock"], {}
        out, rc = self.run("tlx", d, "FuzzS", "FuzzS.cfg", flags, env, timeout=300)
        self.note_coverage(mode, out)
        if self.args.keep or rc != 0:
            with open(os.path.join(d, "out.txt"), "w") as f:
                f.write(out)
        if not self.args.keep and rc == 0:
            shutil.rmtree(d, ignore_errors=True)
        return out, rc, lines


def state_error(out):
    """The core of an error message of a state-level run."""
    e = norm_error(out)
    if e is None:
        return None
    e = re.sub(r"^TLC threw an unexpected exception\..*?The exception was a \S+ ?: ?", "", e)
    for stop in (" While working on", " The error occurred", " \\d+ states generated", " Error: The behavior up to"):
        e = re.split(stop, e)[0]
    return e.strip()


def paranoid_text(out):
    i = out.find("TLX_PARANOID")
    return re.sub(r"\s+", " ", out[i:i + 400])


def describe_state_failure(out):
    if "TLX_PARANOID" in out:
        return "paranoid: " + paranoid_text(out)
    if m := re.search(r"Invariant (Inv_\d+) is violated", out):
        st = out[m.end():m.end() + 600]
        return f"value differs ({m.group(1)} violated): " + re.sub(r"\s+", " ", st)[:400]
    msg = norm_error(out)
    if msg is not None:
        return "error: " + msg[:400]
    if "@@TIMEOUT@@" in out:
        return "timeout"
    return "failed: " + re.sub(r"\s+", " ", out[-400:])
