#!/usr/bin/env python3
"""Differential test: run tlx on corpus models and compare with the TLC oracle
(corpus/out/tlc.json, produced by `scripts/corpus.py tlc`).

usage: scripts/difftest.py [--filter SUBSTR] [--ids ID,ID...] [-j N] [--timeout S] [--workers N] [--release | --profile P]
                           [--extra=ARGS] [--tag TAG] [--failed] [--recover N] [--oracle NAME,...] [--tiers T,...]
                           [--default-mode] [-v]

  --filter SUBSTR  only models whose id contains SUBSTR
  --ids ID,ID...   only these models (exact ids)
  -j N             models run in parallel (default 3)
  --timeout S      per-model timeout in seconds (default 120)
  --workers N      tlx -workers (default 1)
  --release        build and run the release binary (default: the fast
                   test-release profile, or $TLX_BUILD_PROFILE)
  --profile P      build and run target/<P>/tlx of cargo profile P
  --extra=ARGS     extra tlx arguments, e.g. --extra=-compile
  --tag TAG        results file corpus/out/tlx<TAG>.json (default: "", or
                   -wN with --workers N)
  --failed         only rerun the models that failed in the previous run
  --recover N      checkpoint/recover: end each run right after its
                   N-th checkpoint (TLX_CHECKPOINT_ABORT=N; a checkpoint
                   whenever another third of the states TLC found is found:
                   TLX_CHECKPOINT_STATES), then continue it with -recover;
                   the recovered run's results are compared (a run that
                   ends before its N-th checkpoint is compared as it is)
  --oracle NAME,... also the models of these oracle files corpus/out/NAME.json
                   (e.g. tlc-ext-large: the external models of the large
                   tier; the external oracle tlc-ext is read whenever
                   --filter or --ids names ext/ models)
  --tiers T,...    external models: only those of these tiers of corpus/external.json
                   (small, medium, large, huge; the Examples models have no tier and stay)
  --default-mode   run tlx in its default mode and tier (without -tlc, which
                   the parity runs pass) and compare by verdict: the result,
                   the violated names and the fields the optimizations it
                   used preserve (suite corpus-default)
  -v               print the error output of failing models
Environment variables (e.g. TLX_PARANOID=1) are passed on to tlx.

Compares contract tier 1 (docs/compat.md §0): result kind and, for a
safety/property violation, the name of the violated invariant/property. A
liveness failure only needs tlx to name one. Tier 2: counts and depth for
completed searches and deterministic single-worker failures (liveness
failure statistics depend on TLC's periodic check timing).
Multi-worker early-stop depth is scheduling-dependent (D-3).
"""
import tempdirs

import argparse, json, os, shutil, subprocess, sys, tempfile, time
from concurrent.futures import ThreadPoolExecutor

from tlxlib import CaseResult, add_result_args, finish_cases, case_summary, judge_case, load_expected_cases, ROOT, add_profile_args, build_tlx, profile_of, run_proc, parity_args, parity_env, tlx_env
from harness.storage import locked_json_update
from contract import compare as compare_contract
from oracle import select_models, issue as oracle_issue
from tlc import DEFAULT_JOBS, parse, model_scratch_dir

# The exit status of a run ended right after a checkpoint by
# TLX_CHECKPOINT_ABORT (tlx_runtime::explore::checkpoint::ABORT_EXIT).
ABORT_EXIT = 134


def run_tlx(binary, m, workers, timeout, extra=(), recover=0, default_mode=False):
    """Run tlx on model `m`; with `recover`, end the run after its
    `recover`-th checkpoint and continue it with -recover (see --recover);
    with `default_mode`, in tlx's default mode and tier (no -tlc)."""
    if not recover or m.get("flags"):
        return run_once(binary, m, workers, timeout, extra, default_mode=default_mode)
    md = tempdirs.mkdtemp(prefix="tlx-recover-")
    try:
        every = max(1, (m["tlc"].get("distinct") or 3) // 3)
        env = {"TLX_CHECKPOINT_STATES": str(every), "TLX_CHECKPOINT_ABORT": str(recover)}
        t0 = time.time()
        r = run_once(binary, m, workers, timeout, [*extra, "-metadir", os.path.join(md, "md")], env)
        if r.get("exit") != ABORT_EXIT:
            return r
        left = max(1, timeout - (time.time() - t0))
        r = run_once(binary, m, workers, left, [*extra, "-recover", os.path.join(md, "md")])
        r["recovered"] = True
        r["wall_s"] = round(time.time() - t0, 3)
        return r
    finally:
        shutil.rmtree(md, ignore_errors=True)


def run_once(binary, m, workers, timeout, extra=(), extra_env=None, default_mode=False):
    args = [*m.get("flags", []), *extra] if default_mode else parity_args(binary, [*m.get("flags", []), *extra])
    cmd = [binary, "check", m["tla"], "-config", m["cfg"], "-workers", str(workers), *args]
    env = tlx_env(classpath=True, extra={**parity_env(args), **(extra_env or {})})
    t0 = time.time()
    # (A model that writes files runs in a copy of its directory; every other
    # model runs in the directory its inputs are relative to, as the TLC oracle
    # does - see tlxlib.model_cwd.)
    with model_scratch_dir(m) as cwd:
        try:
            p = run_proc(cmd, timeout, cwd=cwd, env=env, grace=5)
        except subprocess.TimeoutExpired:
            return {"result": "timeout", "wall_s": timeout}
        out = p.stdout + p.stderr
        r = parse(out, time.time() - t0, p.returncode)
        if r["result"] in ("error", "unknown"):
            r["error"] = out[-600:]
        return r


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--filter", default="")
    ap.add_argument("--ids", default="", help="comma-separated model ids to run (exact match)")
    ap.add_argument("-j", "--jobs", dest="j", type=int, default=DEFAULT_JOBS)
    ap.add_argument("--timeout", type=int, default=120)
    ap.add_argument("--workers", type=int, default=1)
    add_result_args(ap)
    add_profile_args(ap)
    ap.add_argument("-v", action="store_true", help="print error output of failing models")
    ap.add_argument("--failed", action="store_true", help="only rerun models that failed in the previous run")
    ap.add_argument("--extra", default="", help="extra tlx arguments, e.g. '-compile'")
    ap.add_argument("--tag", default="", help="suffix for the results file (corpus/out/tlx<tag>.json)")
    ap.add_argument("--recover", type=int, default=0, metavar="N", help="end each run after its N-th checkpoint and recover it")
    ap.add_argument("--oracle", default="", metavar="NAME,...", help="also the models of these oracle files corpus/out/NAME.json")
    ap.add_argument("--tiers", default="", metavar="T,...", help="external models: only those of these tiers")
    ap.add_argument("--default-mode", action="store_true",
                    help="run tlx in its default mode and tier (no -tlc) and compare by verdict: the result, the violated "
                         "names and what the optimizations used preserve (docs/reductions.md §1.1)")
    a = ap.parse_args()
    with build_tlx(profile=profile_of(a), prefix="tlx-difftest-") as binary:
        report = run_models(a, binary)
    sys.exit(report["exit_code"])


def run_models(a, binary):
    oracle = select_models(a.filter, a.ids, a.oracle, getattr(a, "tiers", ""))
    models = [m for _, m in sorted(oracle.items())]
    if tiers := {t for t in getattr(a, "tiers", "").split(",") if t}:
        models = [m for m in models if m.get("tier", next(iter(tiers))) in tiers]
    tag = a.tag or ("" if a.workers == 1 else f"-w{a.workers}")
    out_path = os.path.join(ROOT, "corpus", "out", f"tlx{tag}.json")
    prev = {}
    if os.path.exists(out_path):
        with open(out_path) as source:
            prev = json.load(source)
    if a.failed:
        models = [m for m in models if not prev.get(m["id"], {}).get("ok")]

    entries = load_expected_cases()
    suite = a.suite or ("corpus-default" if getattr(a, "default_mode", False) else "corpus-compile" if "-compile" in a.extra.split() else "corpus-interp")
    if not a.suite and a.workers != 1:
        suite += f"-w{a.workers}"

    def one(m):
        problem = m.get("selection_error") or oracle_issue(m)
        if problem:
            print(f'FAIL {m["id"]:58} oracle: {problem}', flush=True)
            return m["id"], {"ok": False, "tlc": m["tlc"], "tlx": {}, "oracle_error": problem}, CaseResult(
                m["id"], "error", {"message": problem, "checked": []})
        execution_error = None
        try:
            r = run_tlx(binary, m, a.workers, a.timeout, a.extra.split(), a.recover, getattr(a, "default_mode", False))
        except Exception as e:  # e.g. a missing spec directory: one failure, not a crashed run
            execution_error = repr(e)
            r = {"result": "error", "error": execution_error}
        t = m["tlc"]
        flags = [*m.get("flags", []), *a.extra.split()]
        comparison = compare_contract(t, r, workers=a.workers,
                                      oracle_workers=m.get("provenance", {}).get("workers", 1),
                                      simulation=any(f in ("-simulate", "-generate") for f in flags),
                                      asymmetric_symmetry=m.get("asymmetric_symmetry", False), optimized=getattr(a, "default_mode", False))
        ok = not comparison.differs
        names = f' checked={",".join(comparison.checked)}'
        if comparison.differs:
            names += f' differs={",".join(comparison.differs)}'
        status = "pass" if ok else "fail"
        if execution_error or r.get("result") in (None, "unknown") or r.get("exit", 0) < 0 or (r.get("result") == "success" and r.get("exit", 0) != 0):
            status, ok = "error", False
        elif r.get("result") == "timeout":
            status = "timeout"
        case = judge_case(CaseResult(m["id"], status, {"kind": r.get("result"), "message": execution_error or r.get("error", ""), **comparison.details()}), entries, suite)
        status = "PASS" if case.status in ("pass", "xpass") else "FAIL"
        line = (f'{status} {m["id"]:58} tlc={t.get("result")}/{t.get("distinct")}/{t.get("generated")}/{t.get("depth")} '
                f'tlx={r.get("result")}/{r.get("distinct")}/{r.get("generated")}/{r.get("depth")} '
                f'({t.get("wall_s")}s vs {r.get("wall_s", 0):.2f}s){names}{" recovered" if r.get("recovered") else ""}')
        if r.get("optimizations"):
            line += f' optimizations={"; ".join(r["optimizations"])}'
        if case.status in ("xfail", "xpass"):
            line += f" [{case.status.upper()}: {case.details['expected']}]"
        case = CaseResult(case.id, case.status, {**case.details, "message": line})
        print(line, flush=True)
        if not ok and a.v and r.get("error"):
            print("    " + r["error"].strip().replace("\n", "\n    "))
        return m["id"], {"ok": ok, "tlx": r, "tlc": t, **comparison.details()}, case

    with ThreadPoolExecutor(a.j) as ex:
        rows = list(ex.map(one, models))
    res = {tid: data for tid, data, _ in rows}
    gaps = sum("oracle_error" in data for _, data, _ in rows)
    legacy = sum(not m.get("provenance") for m in models)
    coverage = {"to_test": len(models), "verified": len(rows) - gaps,
                "oracle_errors": gaps, "legacy_provenance": legacy, "partial": bool(gaps or legacy)}
    print("coverage: " + ", ".join(f"{k}={v}" for k, v in coverage.items()))
    report = finish_cases([case for _, _, case in rows], [m["id"] for m in models], a.results_json, coverage=coverage)
    npass = sum(r["ok"] for r in res.values())
    recovered = f', {sum(bool(r["tlx"].get("recovered")) for r in res.values())} recovered from a checkpoint' if a.recover else ""
    print(f"\n{npass}/{len(res)} passed (this run){recovered}")
    # Re-read under the publication lock: prev only selected --failed cases.
    res = locked_json_update(out_path, lambda current: {**current, **res}, indent=1)
    print(f"{sum(r['ok'] for r in res.values())}/{len(res)} passed (cumulative)")
    print(case_summary(report))
    return report


if __name__ == "__main__":
    main()
