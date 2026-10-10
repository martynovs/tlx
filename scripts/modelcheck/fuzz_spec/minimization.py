"""Ordered structural/expression shrinking and bounded finding minimization."""
import json
import os
import random
import shutil
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor

from modelcheck.fuzz_expr import shrink_candidates, shrink_ty
from modelcheck.results import must
from tlxlib import ROOT
from .artifacts import save_regression
from .constants import FILTER_TIMEOUT
from .expressions import StateGen, subst
from .judgment import brief, judge
from .render import features, render, write_spec
from .runner import mode_variant, run_tlc, run_tlx

def signature(mode, diffs):
    special = [k for k in ("paranoid", "crash", "timeout") if k in diffs]
    return (mode, special[0] if special else ("result" if "result" in diffs else diffs[0]))


def literal_of(t):
    g = StateGen(random.Random(0))
    return g.literal(shrink_ty(t))


def slots(s):
    """The expression slots of `s`: [(key, Node)]."""
    out = [(("init", v), n) for v, (_, n) in s.init.items()]
    for i, a in enumerate(s.actions):
        out += [(("guard", i, j), g) for j, g in enumerate(a.guards)]
        out += [(("upd", i, v), n) for v, (_, n) in a.updates.items()]
        if a.ite:
            out.append((("cond", i), a.ite[0]))
            out += [(("upd2", i, v), n) for v, (_, n) in a.ite[1].items()]
        if a.param:
            out.append((("pset", i), a.param[2]))
        if a.inner:
            out.append((("qset", i), a.inner[2]))
    out += [(("inv", i), n) for i, (_, n) in enumerate(s.invariants)]
    out += [(("aprop", i), n) for i, (_, _, n) in enumerate(s.aprops)]
    for k in ("constraint", "aconstraint", "view"):
        if getattr(s, k) is not None:
            out.append(((k,), getattr(s, k)))
    if s.live:
        out += [(("liveP",), s.live[1]), (("liveQ",), s.live[2])]
    return out


def set_slot(s, key, n):
    k = key[0]
    if k == "init":
        s.init[key[1]] = (s.init[key[1]][0], n)
    elif k == "guard":
        s.actions[key[1]].guards[key[2]] = n
    elif k == "upd":
        a = s.actions[key[1]]
        a.updates[key[2]] = (a.updates[key[2]][0], n)
    elif k == "upd2":
        a = s.actions[key[1]]
        a.ite[1][key[2]] = (a.ite[1][key[2]][0], n)
    elif k == "cond":
        a = s.actions[key[1]]
        a.ite = (n, a.ite[1])
    elif k == "pset":
        a = s.actions[key[1]]
        a.param = (a.param[0], a.param[1], n)
    elif k == "qset":
        a = s.actions[key[1]]
        a.inner = (a.inner[0], a.inner[1], n)
    elif k == "inv":
        s.invariants[key[1]] = (s.invariants[key[1]][0], n)
    elif k == "aprop":
        name, kind, _ = s.aprops[key[1]]
        s.aprops[key[1]] = (name, kind, n)
    elif k in ("constraint", "aconstraint", "view"):
        setattr(s, k, n)
    elif k == "liveP":
        s.live = (s.live[0], n, s.live[2])
    elif k == "liveQ":
        s.live = (s.live[0], s.live[1], n)


def subst_all(s, name):
    for key, n in slots(s):
        if name in n.free:
            set_slot(s, key, subst(n, name, literal_of))


def drop_instance(s):
    s = s.clone()
    s.instance = None
    for a in s.actions:
        a.inst_same = False
        for ups in [a.updates] + ([a.ite[1]] if a.ite else []):
            for v, (kind, n) in list(ups.items()):
                if kind == "iput":
                    ups[v] = ("eq", n)
    subst_all(s, "I!W")
    subst_all(s, "I!W'")
    return s


def drop_var(s, v):
    if s.instance and s.instance[0] == v:
        s = drop_instance(s)
    s = s.clone()
    s.vars = [(x, t) for x, t in s.vars if x != v]
    del s.init[v]
    for a in s.actions:
        a.order = [x for x in a.order if x != v]
        a.updates.pop(v, None)
        if a.ite:
            a.ite[1].pop(v, None)
        for nm in (a.param, a.inner):
            if nm and v in nm[2].free:
                pass
    subst_all(s, v)
    subst_all(s, v + "'")
    for a in s.actions:
        for which in ("param", "inner"):
            p = getattr(a, which)
            if p and v in p[2].free:
                setattr(a, which, (p[0], p[1], literal_of(("set", p[1], 2))))
    return s


def structural_candidates(s):
    """One-step structural reductions of `s`, biggest first."""
    out = []

    def cand(f):
        c = s.clone()
        f(c)
        out.append(c)
    if s.live:
        cand(lambda c: (setattr(c, "live", None), setattr(c, "fairness", [])))
    if s.instance:
        out.append(drop_instance(s))
    for i, (name, *_) in enumerate(s.consts):
        cand(lambda c, i=i, name=name: (c.consts.pop(i), subst_all(c, name)))
    for i, (name, *_) in enumerate(s.overrides):
        cand(lambda c, i=i, name=name: (c.overrides.pop(i), subst_all(c, name)))
    if s.symmetry:
        cand(lambda c: setattr(c, "symmetry", None))
    for k in ("view", "constraint", "aconstraint"):
        if getattr(s, k) is not None:
            cand(lambda c, k=k: setattr(c, k, None))
    for i in range(len(s.invariants)):
        cand(lambda c, i=i: c.invariants.pop(i))
    for i in range(len(s.aprops)):
        cand(lambda c, i=i: c.aprops.pop(i))
    if s.typeok:
        cand(lambda c: setattr(c, "typeok", False))
    for i in range(len(s.fairness)):
        cand(lambda c, i=i: c.fairness.pop(i))
    if len(s.actions) > 1:
        for i in range(len(s.actions)):
            def drop(c, i=i):
                c.actions.pop(i)
                c.fairness = [(k, j if j is None or j < i else j - 1) for k, j in c.fairness if j != i]
            cand(drop)
    if len(s.vars) > 1:
        for v, _ in s.vars:
            out.append(drop_var(s, v))
    if s.deadlock:
        cand(lambda c: setattr(c, "deadlock", False))
    if s.ok_guards:
        cand(lambda c: setattr(c, "ok_guards", False))
    for i, a in enumerate(s.actions):
        if a.param:
            def unparam(c, i=i):
                b = c.actions[i]
                name = b.param[0]
                b.param = None
                subst_all(c, name)
            cand(unparam)
        if a.inner:
            def uninner(c, i=i):
                b = c.actions[i]
                name = b.inner[0]
                b.inner = None
                subst_all(c, name)
            cand(uninner)
        if a.ite:
            cand(lambda c, i=i: setattr(c.actions[i], "ite", None))
        for j in range(len(a.guards)):
            cand(lambda c, i=i, j=j: c.actions[i].guards.pop(j))
        for v in a.order:
            def unchange(c, i=i, v=v):
                b = c.actions[i]
                b.order = [x for x in b.order if x != v]
                b.updates.pop(v)
                if b.ite:
                    b.ite[1].pop(v)
                subst_all(c, v + "'")
            cand(unchange)
            if a.updates[v][0] == "in":
                def to_eq(c, i=i, v=v):
                    b = c.actions[i]
                    b.updates[v] = ("eq", literal_of(c.var_type(v)))
                cand(to_eq)
    for v, (kind, _) in s.init.items():
        if kind == "in":
            def init_eq(c, v=v):
                c.init[v] = ("eq", literal_of(c.var_type(v)))
            cand(init_eq)
    return out


def expression_candidates(s, rng, per_slot=6):
    out = []
    for key, n in slots(s):
        for c in shrink_candidates(n, rng)[:per_slot]:
            t = s.clone()
            set_slot(t, key, c)
            out.append(t)
    return out


def spec_size(s):
    tla, cfg = render(s)
    return len(tla) + len(cfg)


class Shrinker:
    def __init__(self, runner, finding, spec, work, deadline):
        self.runner, self.f, self.work, self.deadline = runner, finding, work, deadline
        self.mode = finding["mode"]
        self.sig = signature(self.mode, finding["diffs"])
        self.spec = spec
        # A compiled-tier mismatch where the interpreter agrees with TLC is
        # shrunk against the interpreter (no TLC run).
        self.against_interp = self.mode != "interp" and finding.get("interp_ok", False)
        self.n = 0
        self.tries = 0

    def reproduces(self, s):
        if time.time() > self.deadline:
            return False
        self.tries += 1
        self.n += 1
        d = os.path.join(self.work, f"try-{self.n}")
        shutil.rmtree(d, ignore_errors=True)
        try:
            write_spec(d, s)
            r = self.runner
            if self.sig[1] in ("paranoid", "crash", "timeout"):
                got = run_tlx(r.binary, d, s.name, self.mode, 60, key=s.variant)
                diffs, _ = judge(d, s.name, {"result": "success"}, got, 4, None)
                return self.sig[1] in diffs
            if self.against_interp:
                want = run_tlx(r.binary, d, s.name, "interp", FILTER_TIMEOUT * 2)
                if want["result"] in ("timeout", None, "unknown"):
                    return False
                interp = True
            else:
                out, rc, _ = run_tlc(d, s.name, 30)
                if out is None or "Semantic errors" in out or "*** Errors:" in out:
                    return False
                want = must(out, rc)
                if want["result"] in (None, "unknown"):
                    return False
                interp = None
            got = run_tlx(r.binary, d, s.name, self.mode, 60, want.get("distinct"), key=s.variant)
            diffs, _ = judge(d, s.name, want, got, mode_variant(s.variant or s.name, self.mode)[2], r.validated, interp=interp)
            return bool(diffs) and signature(self.mode, diffs) == self.sig
        finally:
            shutil.rmtree(d, ignore_errors=True)

    def run(self):
        cur = self.spec
        rng = random.Random(0)
        progress = True
        while progress and time.time() < self.deadline:
            progress = False
            for gen in (structural_candidates, lambda s: expression_candidates(s, rng)):
                cands = sorted(gen(cur), key=spec_size)
                for c in cands:
                    if spec_size(c) >= spec_size(cur):
                        continue
                    if self.reproduces(c):
                        cur = c
                        progress = True
                        break
                if progress:
                    break
        return cur


MODE_ORDER = {"interp": 0, "paranoid": 1, "compile": 2, "recover": 3, "w4": 4}


def shrink_findings(a, runner, findings, out_dir, report):
    """Minimize up to --shrink findings: one per spec (its most telling
    mode: TLC parity of the interpreter first, then the paranoid
    cross-check) and per signature; several at a time (-j), each within
    --shrink-minutes."""
    seen, specs, chosen = set(), set(), []
    for f in sorted(findings, key=lambda f: (MODE_ORDER[f["mode"]], spec_size(f["spec"]))):
        sig = (*signature(f["mode"], f["diffs"]), f["spec"].features.get("heterogeneous", ""))
        entry = {"spec": f["spec"].name, "mode": f["mode"], "diffs": f["diffs"], "note": f["note"][:500], "tlc": f["tlc"],
                 "tlx": f["tlx"], "features": f["features"], "signature": list(sig)}
        report["findings"].append(entry)
        if sig in seen or f["spec"].name in specs or len(chosen) >= a.shrink:
            continue
        seen.add(sig)
        specs.add(f["spec"].name)
        chosen.append((f, entry))
    with ThreadPoolExecutor(max(1, min(a.jobs, len(chosen)))) as ex:
        for text in ex.map(lambda fe: shrink_one(a, runner, *fe, out_dir), chosen):
            print(text, flush=True)


def shrink_one(a, runner, f, entry, out_dir):
    """Minimize finding `f`, write it to OUT/findings/NAME/ (and with --save
    to tests/fuzz/specs); fills entry["shrunk"]; returns a description."""
    t1 = time.time()
    work = tempfile.mkdtemp(prefix="shrink-", dir=out_dir)
    try:
        sh = Shrinker(runner, f, f["spec"], work, time.time() + a.shrink_minutes * 60)
        small = sh.run()
    finally:
        shutil.rmtree(work, ignore_errors=True)
    small = small.clone()
    small.name = f"{f['spec'].name}_{f['mode']}"
    d = os.path.join(out_dir, "findings", small.name)
    shutil.rmtree(d, ignore_errors=True)
    write_spec(d, small)
    out, rc, _ = run_tlc(d, small.name)
    if out is None:
        return f"shrunk {small.name}: TLC timed out on the minimized spec"
    with open(os.path.join(d, "tlc.out"), "w") as fo:
        fo.write(out)
    want = must(out, rc)
    key = f["spec"].variant or f["spec"].name
    got = run_tlx(runner.binary, d, small.name, f["mode"], 120, want.get("distinct"), key=key)
    diffs, note = judge(d, small.name, want, got, mode_variant(key, f["mode"])[2], runner.validated)
    with open(os.path.join(d, f"tlx-{f['mode']}.out"), "w") as fo:
        fo.write(got.get("out", ""))
    flags, env, workers = mode_variant(key, f["mode"])
    seed, index = f["spec"].name[1:].split("_")
    info = {"found": f"fuzz_spec.py --seed {seed} --start {index} --count 1", "mode": f["mode"],
            "flags": flags + ["-workers", str(workers)], "env": env, "diffs": diffs, "note": note[:1000],
            "tlc": brief(want), "tlx": brief(got), "original_diffs": f["diffs"], "features": features(small),
            "shrink_tries": sh.tries, "shrink_secs": round(time.time() - t1), "variant": key}
    with open(os.path.join(d, "finding.json"), "w") as fo:
        json.dump(info, fo, indent=1, sort_keys=True)
    entry["shrunk"] = {"dir": os.path.relpath(d, ROOT), "reproduces": bool(diffs), **info}
    text = (f"\nshrunk {f['spec'].name}/{f['mode']} in {time.time() - t1:.0f}s ({sh.tries} tries)"
            f" -> {os.path.relpath(d, ROOT)}: reproduces={bool(diffs)} {diffs}\n{render(small)[0]}")
    if a.save and diffs:
        save_regression(d, small.name, info)
        text += f"saved tests/fuzz/specs/{small.name}"
    return text
