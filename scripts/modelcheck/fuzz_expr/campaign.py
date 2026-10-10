"""Differential fuzzing of TLA+ expression semantics: TLC vs tlx (interpreter,
-compile).

usage: scripts/fuzz_expr.py [--seed N] [--count N] [--modes tlc,interp,compile]
                            [--batch N] [--jobs N] [--shrink N] [--out DIR] [--save]
       scripts/fuzz_expr.py --replay FILE.jsonl [--modes ...]
       scripts/fuzz_expr.py --show --seed N --count N      (print the expressions)

Modes:
  tlc      constant level: `ASSUME Start <= i => PrintT(<<i, ToString(e_i)>>)`
           per expression, evaluated by TLC (the oracle).
  interp   the same spec with the tlx interpreter; compared with TLC (the
           value printed by ToString, which shows TLC's normalized order, or
           the error message).
  compile  state level: each expression is embedded in a spec whose closed
           literal subterms are partly lifted into variables (so that they are
           not constant-folded) and is evaluated in Init (`r_i = e_i`), in
           Next (`r_i' = e_i`) or in an invariant (`e_i = EXP_i`), where
           EXP_i is the expected value (TLC's, else the interpreter's). The
           invariant `r_i = EXP_i` checks the value. `-compile` runs with
           TLX_PARANOID=1 (every successor and invariant cross-checked
           against the interpreter), a tiny pre-run and the native code built
           before the search. Expressions that fail in constant mode are run
           one per spec and must also fail; error messages and exit codes
           are informational (compatibility tier 3).

TLC stops at the first failing ASSUME: the spec text is kept identical and the
run is restarted with the CONSTANT Start after the failing expression (so the
intern order of strings, which decides set order and CHOOSE, does not change).
All strings an expression can produce are interned up front by a `Pool`
definition (in a random order: string order is the intern order, not the
lexicographic one).

Each discrepancy is minimized (greedy one-step shrinking: replace a node by a
same-typed descendant or a minimal literal while the discrepancy persists)
and written to DIR/findings.jsonl (with --save also appended to
tests/fuzz/findings.jsonl, which --replay re-checks).
Default DIR: target/fuzz/seed-N. At most 2 TLC JVMs run at a time.
"""


import argparse
import json
import os
import random
import re
import shutil
import sys
import time
from concurrent.futures import ThreadPoolExecutor

from tlxlib import DEFAULT_JOBS, ROOT, add_profile_args, build_tlx, profile_of

from .generation import Gen
from .hetero import known_expression
from .node import node_from_json
from .render import pool_def
from .runner import Runner
from .serializer import INFO_KINDS, classify, info_key
from .shrink import shrink_candidates


# ---------------------------------------------------------------------------
# The fuzzing loop
# ---------------------------------------------------------------------------


def batched(xs, n):
    return [xs[i:i + n] for i in range(0, len(xs), n)]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--count", type=int, default=500)
    ap.add_argument("--modes", default="tlc,interp,compile",
                    help="comma list of tlc, interp, compile, compile-ir (compile with TLX_NO_NATIVE)")
    ap.add_argument("--batch", type=int, default=150, help="expressions per constant-level spec")
    ap.add_argument("--sbatch", type=int, default=25, help="expressions per state-level spec")
    ap.add_argument("-j", "--jobs", type=int, default=DEFAULT_JOBS, help="parallel batches (TLC itself is limited to 2 JVMs)")
    ap.add_argument("--shrink", type=int, default=12, help="minimize at most N discrepancies")
    ap.add_argument("--p-unsafe", type=float, default=0.06, help="probability of error-prone variants")
    ap.add_argument("--out", default=None)
    ap.add_argument("--save", action="store_true", help="append minimized findings to tests/fuzz/findings.jsonl")
    ap.add_argument("--replay", default=None, help="re-check the cases of a findings .jsonl file")
    ap.add_argument("--show", action="store_true", help="only print the generated expressions")
    ap.add_argument("--keep", action="store_true", help="keep the generated specs")
    ap.add_argument("-v", "--verbose", action="store_true", help="list the informational discrepancies")
    ap.add_argument("--no-build", action="store_true")
    add_profile_args(ap)
    a = ap.parse_args()
    modes = a.modes.split(",")
    rng = random.Random(a.seed)
    gen = Gen(rng, p_unsafe=a.p_unsafe)
    cases = []
    if a.replay:
        with open(a.replay) as stream:
            cases = [json.loads(line) for line in stream if line.strip()]
        nodes = [node_from_json(c["node"]) for c in cases]
        for c in cases:
            gen.pool |= set(c.get("pool", []))
    else:
        nodes = [gen.top() for _ in range(a.count)]
    if a.show:
        for n in nodes:
            print(n.render())
        return
    with build_tlx(profile=profile_of(a), prefix="tlx-fuzz-", build=not a.no_build) as binary:
        run_fuzz(a, modes, gen, nodes, cases, binary)


def run_fuzz(a, modes, gen, nodes, cases, binary):
    out_dir = a.out or os.path.join(ROOT, "target", "fuzz", f"seed-{a.seed}" if not a.replay else "replay")
    work = os.path.join(out_dir, "work")
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work, exist_ok=True)
    runner = Runner(a, work, binary)
    pool_line = pool_def(gen.pool, random.Random(a.seed))
    if a.replay and cases[0].get("pool_line"):
        # The recorded string order (missing strings appended).
        pool_line = cases[0]["pool_line"]
        missing = gen.pool - set(json.loads("[" + pool_line[pool_line.index("<<") + 2:pool_line.rindex(">>")] + "]"))
        if missing:
            pool_line = pool_line.rstrip()[:-2] + "".join(", " + json.dumps(m) for m in sorted(missing)) + ">>\n"
    texts = [n.render() for n in nodes]
    t0 = time.time()

    # Constant level.
    results = {m: {} for m in ("tlc", "interp") if m in modes or m == "interp"}
    batches = batched(list(range(len(nodes))), a.batch)

    def const_job(kind, b):
        r = runner.eval_const(kind, [texts[i] for i in b], pool_line)
        return kind, b, r

    jobs = [(k, b) for b in batches for k in results]
    with ThreadPoolExecutor(a.jobs) as ex:
        for kind, b, r in ex.map(lambda kb: const_job(*kb), jobs):
            for j, i in enumerate(b):
                results[kind][i] = r.get(j)
    print(f"constant level: {len(nodes)} expressions, {time.time() - t0:.0f}s, {runner.stats}", flush=True)
    findings = []
    info = {}
    if "tlc" in results:
        for i in range(len(nodes)):
            k = classify(results["tlc"].get(i), results["interp"].get(i))
            if k in INFO_KINDS:
                info.setdefault(k, {}).setdefault(info_key(k, results["tlc"][i], results["interp"][i]), (i, texts[i], results["tlc"][i], results["interp"][i]))
            elif k:
                findings.append({"layer": "interp-vs-tlc", "kind": k, "i": i, "expr": texts[i],
                                 "tlc": results["tlc"][i], "tlx": results["interp"][i]})

    # State level.
    smodes = [m for m in modes if m in ("compile", "compile-ir")]
    srng = random.Random(a.seed + 1)
    items = []
    for i, n in enumerate(nodes):
        ref = results.get("tlc", {}).get(i) or results["interp"].get(i)
        if ref is None or ref[0] == "timeout" or "tlc" in results and classify(results["tlc"].get(i), results["interp"].get(i)) not in (None, "message"):
            continue
        if ref[0] == "ok" and not expressible(ref[1]):
            continue
        ctx, seed = srng.choice(["init", "next", "next", "inv"]), srng.randrange(1 << 30)
        if a.replay and "ctx" in cases[i]:
            ctx, seed = cases[i]["ctx"], cases[i]["seed"]
        items.append((i, (n, ctx, ref[1] if ref[0] == "ok" else None, ref[1] if ref[0] == "err" else None, seed)))
    for mode in smodes:
        t1 = time.time()
        ok_items = [it for it in items if it[1][2] is not None]
        err_items = [it for it in items if it[1][2] is None]
        sb = batched(ok_items, a.sbatch) + [[it] for it in err_items]

        def state_job(b):
            return b, runner.eval_state(mode, [it[1] for it in b], pool_line)
        if mode in runner.stats:
            runner.stats[mode] = {}
        with ThreadPoolExecutor(a.jobs) as ex:
            for b, r in ex.map(state_job, sb):
                for j, (i, it) in enumerate(b):
                    if r.get(j):
                        findings.append({"layer": f"{mode}-vs-interp", "kind": "state", "i": i, "expr": texts[i], "ctx": it[1],
                                         "problem": r[j], "seed": it[4]})
        print(f"state level ({mode}): {len(items)} expressions ({len(err_items)} erroring), {time.time() - t1:.0f}s, {runner.stats}",
              flush=True)

    for f in findings:
        f["node"] = nodes[f["i"]].to_json()
        if issue := known_expression(f):
            f["known"] = issue

    # Minimize.
    shrunk = []
    seen_sig = set()
    # Compiled-tier findings first: they are cheap to minimize.
    for f in sorted(findings, key=lambda f: f["layer"] == "interp-vs-tlc"):
        sig = (f["layer"], f["kind"], f.get("problem", "")[:40] if f["layer"] != "interp-vs-tlc" else
               (f["tlc"][1][:60] if f["tlc"][0] == "err" else f["tlx"][1][:60] if f["tlx"][0] == "err" else ""))
        if len(shrunk) >= a.shrink or sig in seen_sig:
            f["shrunk"] = None
            continue
        seen_sig.add(sig)
        t1 = time.time()
        small = minimize(runner, f, nodes[f["i"]], pool_line, a)
        f["min"] = small
        shrunk.append(f)
        print(f"  shrunk #{f['i']} ({f['layer']}/{f['kind']}) in {time.time() - t1:.0f}s: {small['expr']}", flush=True)

    with open(os.path.join(out_dir, "findings.jsonl"), "w") as fo:
        for f in findings:
            rec = dict(f)
            rec["node"] = nodes[f["i"]].to_json()
            rec["pool"] = sorted(gen.pool)
            rec["pool_line"] = pool_line
            fo.write(json.dumps(rec) + "\n")
    if a.save and shrunk:
        os.makedirs(os.path.join(ROOT, "tests", "fuzz"), exist_ok=True)
        with open(os.path.join(ROOT, "tests", "fuzz", "findings.jsonl"), "a") as fo:
            for f in shrunk:
                rec = {"layer": f["layer"], "kind": f["kind"], "expr": f["min"]["expr"], "node": f["min"]["node"],
                       "pool": sorted(gen.pool), "pool_line": pool_line, "detail": f["min"]["detail"], "fuzz_seed": a.seed}
                if "ctx" in f:
                    rec["ctx"], rec["seed"] = f["ctx"], f["seed"]
                fo.write(json.dumps(rec) + "\n")
    nerr = sum(1 for v in results["interp"].values() if v and v[0] == "err")
    print(f"\n{len(nodes)} expressions ({nerr} erroring in the interpreter), {len(findings)} discrepancies, "
          f"{time.time() - t0:.0f}s, {runner.stats}")
    by = {}
    for f in findings:
        by.setdefault((f["layer"], f["kind"]), []).append(f)
    for (layer, kind), fs in sorted(by.items()):
        print(f"  {layer} {kind}: {len(fs)}")
    for k, d in sorted(info.items()):
        print(f"  (info) {k}: {sum(1 for _ in d)} distinct")
        if a.verbose:
            for (i, e, rt, rx) in d.values():
                print(f"      #{i}: TLC {rt[1]!r} / tlx {rx[1]!r}")
    for f in shrunk:
        print(f"\n[{f['layer']} / {f['kind']}] {f['min']['expr']}\n    {f['min']['detail']}")
    print(f"\nfindings: {os.path.join(out_dir, 'findings.jsonl')}")
    if not a.keep:
        shutil.rmtree(work, ignore_errors=True)
    unexpected = sum(not f.get("known") for f in findings)
    print(f"judgment: {unexpected} unexpected, {len(findings) - unexpected} expected discrepancies")
    sys.exit(1 if unexpected else 0)


def expressible(text):
    """Whether a printed value can be read back as a TLA+ expression."""
    return not re.search(r"\b(Nat|Int|STRING|Seq)\b|<<.*>> :>", text) and "LAMBDA" not in text


def minimize(runner, f, node, pool_line, a):
    """Greedy one-step shrinking while the same kind of discrepancy remains."""
    rng = random.Random(0)
    layer, kind = f["layer"], f["kind"]
    cur, cur_detail = node, None

    def check_tlc(cands):
        texts = [c.render() for c in cands]
        rt = runner.eval_const("tlc", texts, pool_line, tag="shrink")
        ri = runner.eval_const("interp", texts, pool_line, tag="shrink")
        out = []
        for j in range(len(cands)):
            k = classify(rt.get(j), ri.get(j))
            out.append((k == kind, f"TLC: {rt.get(j)} / tlx: {ri.get(j)}"))
        return out

    def check_state(cands):
        mode = layer.split("-vs-")[0]
        texts = [c.render() for c in cands]
        ri = runner.eval_const("interp", texts, pool_line, tag="shrink")
        out = []
        items = []
        for j, c in enumerate(cands):
            r = ri.get(j)
            if r is None or r[0] == "timeout" or (r[0] == "ok" and not expressible(r[1])):
                items.append(None)
                continue
            items.append((c, f["ctx"], r[1] if r[0] == "ok" else None, r[1] if r[0] == "err" else None, f["seed"]))
        for j, it in enumerate(items):
            if it is None:
                out.append((False, ""))
                continue
            res = runner.eval_state(mode, [it], pool_line, tag="shrink")
            out.append((bool(res.get(0)), f"interp: {ri.get(j)}; {mode}: {res.get(0)}"))
        return out

    check = check_tlc if layer == "interp-vs-tlc" else check_state
    base_res = check([cur])[0]
    if not base_res[0]:
        return {"expr": cur.render(), "node": cur.to_json(), "detail": "not reproducible: " + base_res[1]}
    cur_detail = base_res[1]
    for _ in range(40):
        cands = shrink_candidates(cur, rng)[:40 if layer == "interp-vs-tlc" else 15]
        if not cands:
            break
        res = check(cands)
        hit = next((j for j, r in enumerate(res) if r[0]), None)
        if hit is None:
            break
        cur, cur_detail = cands[hit], res[hit][1]
    return {"expr": cur.render(), "node": cur.to_json(), "detail": cur_detail}
