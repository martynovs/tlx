"""Bounded campaign admission, result aggregation and expression coordination."""
import json
import os
import shutil
import sys
import tempfile
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed

from modelcheck import fuzz_expr as fx
from tlxlib import (ROOT, CaseResult, case_summary, finish_cases, judge_case,
                    load_expected_cases, profile_of, run_proc, tlx_env)
from .artifacts import regression_names, write_report
from .generation import SpecGen
from .minimization import shrink_findings
from .render import features
from .replay import replay
from .runner import Runner

# Open expression bugs (tests/fuzz/expr-open.jsonl; remove an entry once fixed): (id, predicate(finding)).
KNOWN_EXPR = []


def run(a, seed, modes, out_dir, binary):
    t0 = time.time()
    runner = Runner(a, binary, out_dir)
    entries = load_expected_cases()
    suite = a.suite or "fuzz"
    cases, selected = [], []
    scratch = tempfile.mkdtemp(prefix="work-", dir=out_dir)
    report = {"seed": seed, "modes": modes, "specs": [], "findings": [], "stats": runner.stats}
    try:
        if a.replay or a.replay_only is not None:
            names = a.replay_only or regression_names()
            if a.replay_only and (bad := [n for n in names if n not in regression_names()]):
                print(f"unknown regressions: {bad}", file=sys.stderr)
                return 2
            for c in replay(runner, names, modes, scratch):
                c = judge_case(c, entries, suite)
                cases.append(c)
                selected.append(c.id)
        if a.replay_only is None:
            gen_cases, findings = generate(a, seed, modes, out_dir, runner, report, t0, scratch)
            for c in gen_cases:
                if c.status != "xfail":
                    c = judge_case(c, entries, suite)
                cases.append(c)
                selected.append(c.id)
            shrink_findings(a, runner, findings, out_dir, report)
        if a.expr:
            c = expr_case(a, seed, out_dir, binary)
            cases.append(c)
            selected.append(c.id)
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
    report["secs"] = round(time.time() - t0)
    write_report(out_dir, report, cases)
    result = finish_cases(cases, selected, a.results_json)
    print(case_summary(result))
    return result["exit_code"]


def generate(a, seed, modes, out_dir, runner, report, t0, scratch):
    """Generate and check specs: ([CaseResult], [finding])."""
    deadline = t0 + a.minutes * 60 if a.minutes else None
    cases, findings = [], []
    lock = threading.Lock()

    def job(i):
        if deadline and time.time() > deadline:
            return None
        s = SpecGen(seed, i).spec()
        d = os.path.join(scratch, s.name)
        try:
            res = runner.check(s, d, modes)
        except Exception as error:  # noqa: BLE001 - one broken spec must not stop the campaign
            res = {"name": s.name, "features": features(s), "status": "error", "why": repr(error), "modes": {}}
        if res["status"] in ("fail", "error") or a.keep:
            dest = os.path.join(out_dir, "specs", s.name)
            shutil.rmtree(dest, ignore_errors=True)
            if os.path.isdir(d):
                shutil.copytree(d, dest, ignore=shutil.ignore_patterns("tlc-states-*", "recover-*"))
        shutil.rmtree(d, ignore_errors=True)
        return s, res

    indices = iter(range(a.start, a.start + a.count))
    done = 0
    with ThreadPoolExecutor(a.jobs) as ex:
        # (At most 2 jobs per worker in flight: a campaign's --count is a bound, not a list.)
        pending = set()

        def refill():
            while len(pending) < 2 * a.jobs and not (deadline and time.time() > deadline):
                i = next(indices, None)
                if i is None:
                    return
                pending.add(ex.submit(job, i))

        refill()
        while pending:
            fut = next(as_completed(pending))
            pending.discard(fut)
            refill()
            got = fut.result()
            if got is None:
                continue
            s, res = got
            done += 1
            with lock:
                report["specs"].append({k: res.get(k) for k in ("name", "status", "why", "result", "features", "tlc")})
            runner.bump(f"status {res['status']}")
            if res.get("result"):
                runner.bump(f"tlc {res['result']}")
            status = res["status"]
            cid = f"gen/{s.name}"
            if status == "skip":
                cases.append(CaseResult(cid, "skip", {"message": res.get("why", "")[:300]}))
            elif status == "error":
                cases.append(CaseResult(cid, "error", {"message": res.get("why", "")[:500]}))
                print(f"ERROR {cid}: {res.get('why')}", flush=True)
            else:
                failing = {m: r for m, r in res["modes"].items() if r["diffs"] and not r.get("known")}
                known = {m: r for m, r in res["modes"].items() if r.get("known")}
                details = {"kind": res.get("result"), "differs": {m: r["diffs"] for m, r in {**known, **failing}.items()},
                           "features": res["features"]}
                if known and not failing:
                    details["expected"] = "; ".join(sorted({r["known"][1] for r in known.values()}))
                cases.append(CaseResult(cid, "fail" if failing else "xfail" if known else "pass", details))
                shrinkable = {**known, **failing} if a.shrink_known else failing
                if shrinkable:
                    interp_ok = not res["modes"].get("interp", {}).get("diffs")
                    for m, r in shrinkable.items():
                        findings.append({"spec": s, "mode": m, "diffs": r["diffs"], "note": r["note"], "tlc": res["tlc"],
                                         "tlx": r["tlx"], "interp_ok": interp_ok and m != "interp", "features": res["features"]})
                if failing:
                    print(f"FAIL {cid} [{res['features']}] tlc={res['tlc']}\n" + "\n".join(
                        f"     {m}: tlx={r['tlx']} differs: {r['diffs']}" + (f" ({r['note'][:300]})" if r["note"] else "")
                        for m, r in failing.items()), flush=True)
                elif a.verbose:
                    print(f"PASS {cid} [{res['features']}] tlc={res['tlc']}", flush=True)
            if done % 100 == 0:
                # (A campaign cut short still leaves its results.)
                write_report(out_dir, report, cases)
            if done % 25 == 0:
                print(f"  {done} specs, {time.time() - t0:.0f}s, {dict(sorted(runner.stats.items()))}", flush=True)
    print(f"generated {done} specs in {time.time() - t0:.0f}s: {dict(sorted(runner.stats.items()))}", flush=True)
    return sorted(cases, key=lambda c: c.id), findings


def expr_case(a, seed, out_dir, binary):
    """scripts/fuzz_expr.py with the same seed, as one case."""
    d = os.path.join(out_dir, "expr")
    cmd = [sys.executable, os.path.join(ROOT, "scripts", "fuzz_expr.py"), "--seed", str(seed), "--count", str(a.expr),
           "--jobs", str(a.jobs), "--no-build", "--out", d, "--profile", profile_of(a)]
    p = run_proc(cmd, cwd=ROOT, env=tlx_env())
    out = p.stdout + p.stderr
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, "fuzz_expr.log"), "w") as f:
        f.write(out)
    tail = "\n".join(out.strip().splitlines()[-12:])
    print(f"fuzz_expr --seed {seed} --count {a.expr}: exit {p.returncode}\n{tail}", flush=True)
    details = {"kind": "expressions", "message": tail[-1500:], "log": os.path.relpath(os.path.join(d, "fuzz_expr.log"), ROOT)}
    if p.returncode == 0:
        return CaseResult(f"expr/seed-{seed}", "pass", details)
    findings = []
    path = os.path.join(d, "findings.jsonl")
    if os.path.exists(path):
        with open(path) as f:
            findings = [json.loads(line) for line in f if line.strip()]
    known = [known_expr(f) for f in findings]
    if findings and all(known):
        details["expected"] = "; ".join(sorted(set(known)))
        return CaseResult(f"expr/seed-{seed}", "xfail", details)
    return CaseResult(f"expr/seed-{seed}", "fail", details)


def known_expr(f):
    return next((k for k, pred in KNOWN_EXPR if pred(f)), None) or fx.known_expression(f)
