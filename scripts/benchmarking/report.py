"""Pure rendering and comparison of benchmark records.

The passes write history and scaling files; this module turns them into
markdown, tables and verdicts and knows nothing else: `report_markdown` and
`scale_report` render, `counts_check`, `comparison_samples`, `compare_runs` and
`scale_counts` compare. Nothing here reads a file, starts a process or reaches
the entry point -- the passes in `benchmarking/commands/` and the drivers that
produce the records call these, and `scripts/bench.py` re-exports every name so
the callers that reach them through it keep working (HARNESS-03: a domain
library does not import the CLI or a test module; CODE-03: reporting is a
responsibility of its own).
"""
import math

from tlxlib import geomean

# Mode name of the TLC runs in a history file.
TLC_MODE = "tlc"



def counts_check(r, ref, *, asymmetric_symmetry=False):
    """Whether run `r` agrees with the reference result `ref` (TLC; a
    record with result and distinct): the same kind of result, and for a
    complete search (success) the same number of distinct states (a search
    stopped by a violation counts the states found so far, which depends on
    the order of exploration). A run of the default mode that used
    optimizations (a reduced search, `Optimizations:` line) is checked by its
    result only. None if either is missing or did not finish."""
    if not ref or not ok_result(ref) or not ok_result(r):
        return None
    if r.get("result") != ref.get("result"):
        return False
    if r.get("compatibility") is False or r.get("mode") == "default" or r.get("optimizations"):
        return True
    if asymmetric_symmetry and (r.get("workers", 1) != 1 or ref.get("workers", 1) != 1):
        return None
    return r.get("result") != "success" or r.get("distinct") == ref.get("distinct")



def comparison_samples(r, metric):
    """Minimum, relative range (noise), and any reason timings are inconclusive."""
    xs = r.get("walls" if metric == "wall" else "cpu") or []
    valid = [x for x in xs if isinstance(x, (int, float)) and math.isfinite(x) and x > 0]
    lo = min(valid) if valid else float("nan")
    noise = (max(valid) - lo) / lo if valid else float("nan")
    if len(valid) != len(xs):
        reason = "missing/invalid metric samples"
    elif len(valid) < 3:
        reason = f"only {len(valid)} samples (need >= 3)"
    elif metric == "cpu" and len(xs) != len(r.get("walls") or []):
        reason = "missing CPU samples"
    elif not ok_result(r):
        reason = f"unfinished run ({r.get('result')})"
    else:
        reason = ""
    return lo, noise, reason



def compare_runs(ra, rb, *, metric="wall", threshold=0.05, max_noise=0.05, min_delta=0.05, distinct_success_only=False):
    """The comparison of two sets of runs (history `runs` dicts) as data:
    {"rows": [...], "regress": [key], "better": [key], "inconclusive": [key], "per_tier": {tier: [ratio]}}.
    A row has key, a, b (min times), ratio, noise_a, noise_b, flag ("PASS", "faster", "REGRESSION",
    "INCONCLUSIVE") and, for a changed number of distinct states, distinct=(A, B). With `distinct_success_only`
    the distinct states of a run that stopped at a violation are not compared (with several workers the count at
    the stop is scheduling-dependent; the tier-2 contract only fixes it for complete searches)."""
    regress, better, inconclusive, per_tier, rows = [], [], [], {}, []
    for k in sorted(set(ra) | set(rb)):
        if k not in ra or k not in rb:
            inconclusive.append(k)
            rows.append({"key": k, "flag": "INCONCLUSIVE", "reasons": [f"missing run in {'A' if k not in ra else 'B'}"]})
            continue
        x, y = ra[k], rb[k]
        xa, nx, rx = comparison_samples(x, metric)
        ya, ny, ry = comparison_samples(y, metric)
        reasons = [f"{label}: {reason}" for label, reason in (("A", rx), ("B", ry)) if reason]
        if len(x.get("walls") or []) != len(y.get("walls") or []):
            reasons.append("unequal sample counts")
        if max(nx, ny) > max_noise:
            reasons.append("high noise (possible machine contention or intermittent slow path)")
        ratio = ya / xa if xa > 0 else float("nan")
        if reasons:
            flag = "INCONCLUSIVE"
            inconclusive.append(k)
        else:
            flag = "PASS"
            if ratio > 1 + threshold and ya - xa > min_delta:
                flag = "REGRESSION"
                regress.append(k)
            elif ratio < 1 / (1 + threshold) and xa - ya > min_delta:
                flag = "faster"
                better.append(k)
            per_tier.setdefault(x["tier"], []).append(ratio)
        row = {"key": k, "a": xa, "b": ya, "ratio": ratio, "noise_a": nx, "noise_b": ny, "flag": flag, "reasons": reasons}
        if ok_result(x) and ok_result(y) and x.get('result') != y.get('result'):
            row['flag'] = 'REGRESSION'
            row['verdict'] = (x.get('result'), y.get('result'))
            if k not in regress:
                regress.append(k)
        # (Simulations with a fixed seed are deterministic too.)
        count_comparable = all(r.get("compatibility") is not False and r.get("mode") != "default" for r in (x, y))
        if count_comparable and x.get("distinct") != y.get("distinct") and not (distinct_success_only and not (x.get("result") == y.get("result") == "success")):
            row["distinct"] = (x.get("distinct"), y.get("distinct"))
            if k not in regress:
                regress.append(k)
        rows.append(row)
    return {"rows": rows, "regress": regress, "better": better, "inconclusive": inconclusive, "per_tier": per_tier}



def parse_table(text):
    """The model rows of an existing results file: [(model, line)] in order."""
    rows, header = [], False
    for line in text.splitlines():
        if not line.startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if cells and cells[0] == "model":
            header = True
        elif header and cells and cells[0] and not set(cells[0]) <= set("-: "):
            rows.append((cells[0], line))
    return rows



def merge_rows(existing, new, order):
    """`existing` with the rows of `new` ({model: line}) replaced in place and
    the models new to the file (`order`) appended; other rows keep their line
    and position."""
    out, seen = [], set()
    for k, line in existing:
        out.append(new[k] if k in new else line)
        seen.add(k)
    out += [new[k] for k in order if k not in seen]
    return out



def table_columns(col1, w):
    """The column names of the tlx vs TLC table (`col1`: the TLC 1w column)."""
    return ["model", "distinct", col1, f"TLC {w}w", "tlx interp 1w", "tlx compile 1w", f"tlx compile {w}w",
            "tlx jit 1w", "tlx c 1w", "tlx compile 1w cold", "speedup (1w)", f"speedup ({w}w)", "counts"]



def render_row(row):
    """A table line: row = (model, distinct, TLC 1w, TLC Nw, interp 1w, compile 1w,
    compile Nw, jit 1w, c 1w, cold 1w, counts ok); times may be None."""
    k, dd, t1, tn, i1, c1, cn, jit, c, cold, ok = row

    def f(x):
        return "-" if x is None else f"{x:.2f}"
    s1 = f"{t1 / c1:.1f}×" if t1 and c1 else "-"
    sn = f"{tn / cn:.1f}×" if tn and cn else "-"
    return f"| {k} | {dd} | {f(t1)} | {f(tn)} | {f(i1)} | {f(c1)} | {f(cn)} | {f(jit)} | {f(c)} | {f(cold)} | {s1} | {sn} | {'✓' if ok else '✗'} |"



def table_cells(line):
    return [c.strip() for c in line.strip().strip("|").split("|")]



def table_header(text):
    """The column names of the first table of a results file (None if none)."""
    for line in text.splitlines():
        if line.startswith("| model"):
            return table_cells(line)
    return None



def upgrade_rows(rows, header, cols):
    """Rows ([(model, line)]) of a table with columns `header` rewritten for
    the columns `cols` (by name; columns the old table lacks are "-"), e.g.
    rows kept from a table without the tier columns."""
    # (The TLC 1w column's label names its source: the same column.)
    norm = lambda c: "TLC 1w" if c.startswith("TLC 1w") else c
    if not header or list(map(norm, header)) == list(map(norm, cols)):
        return rows
    out = []
    for k, line in rows:
        cells = dict(zip(map(norm, header), table_cells(line)))
        out.append((k, "| " + " | ".join(cells.get(norm(c), "-") for c in cols) + " |"))
    return out



def best(rs):
    """The run with the least wall time (timeouts last)."""
    return min(rs, key=lambda r: (r.get("result") == "timeout", r.get("wall_s") or 0))



def ok_result(r):
    return r.get("result") not in (None, "timeout", "error", "unknown")

# Runs that did not finish (no counts, no time): timed out, or killed by hand
# ("killed", recorded with a note) or by a signal the parser reports as "unknown".
INCOMPLETE = ("timeout", "killed", "unknown")

# Buckets of report_markdown's geometric means: TLC's 1-worker wall time.
TIME_BUCKETS = [(0, 1, "TLC 1w < 1 s"), (1, 10, "TLC 1w 1-10 s"), (10, 60, "TLC 1w 10-60 s"), (60, float("inf"), "TLC 1w >= 60 s")]



def report_markdown(h, slow=3.0, slow_min=2.0):
    """Markdown of a history file with TLC runs (bench.py run --tlc): one row
    per model (min wall times of TLC 1w and Nw, tlx interpreter 1w, -compile
    1w and Nw, the speedups TLC / tlx at the same worker count, and the
    counts check of every tlx run against TLC 1w on the same machine (TLC Nw if 1w timed out), see
    counts_check), the geometric means of the speedups per tier and per TLC
    1w time bucket, and the models where -compile 1w is less than `slow`
    times faster than TLC 1w while TLC takes at least `slow_min` seconds.
    Lines, without a heading."""
    from corpus import external_models, EXTERNAL_TIERS
    asymmetric = {m["id"] for m in external_models(EXTERNAL_TIERS) if m.get("asymmetric_symmetry")}
    runs = h["runs"].values()
    by = {}
    for r in runs:
        by.setdefault(r["model"], {})[r["mode"], r["workers"]] = r
        by[r["model"]].setdefault("tier", r["tier"])
    n = max((w for r in runs for w in [r["workers"]] if r["mode"] == TLC_MODE and w > 1), default=None)

    def t(r):
        if not r:
            return None
        return r["min"] if ok_result(r) else None

    def cell(r):
        if not r:
            return "-"
        if r.get("result") == "timeout":
            return f">{r['min']:.0f}"
        return f"{r['min']:.2f}" if ok_result(r) else r.get("result") or "?"

    def sp(a, b):
        return a / b if a and b else None

    def fx(x):
        return f"{x:.1f}×" if x else "-"

    def spcell(tr, xr):
        # (TLC timed out, tlx finished: a lower bound, not in the means.)
        if tr and xr and tr.get("result") == "timeout" and ok_result(xr):
            return f">{tr['min'] / xr['min']:.1f}×"
        return fx(sp(t(tr), t(xr)))

    cols = ["model", "tier", "result", "distinct", "TLC 1w", f"TLC {n}w", "tlx interp 1w", "tlx compile 1w", f"tlx compile {n}w",
            "interp / TLC 1w", "compile 1w speedup", f"compile {n}w speedup", "counts"]
    lines = ["| " + " | ".join(cols) + " |", "|---|---|---|" + "---:|" * (len(cols) - 4) + ":-:|"]
    rows, tlc_differs = [], []
    for k in sorted(by, key=lambda k: (by[k]["tier"], k)):
        g = by[k]
        tlc1, tlcn = g.get((TLC_MODE, 1)), g.get((TLC_MODE, n))
        i1, c1, cn = g.get(("interp", 1)), g.get(("compile", 1)), g.get(("compile", n))
        # The reference: TLC 1w, else (timed out) TLC Nw.
        tref = next((r for r in ((tlc1,) if k in asymmetric else (tlc1, tlcn)) if r and ok_result(r)), None)
        # (TLC's own multi-worker counts may differ from its 1-worker ones,
        # e.g. with SYMMETRY: listed below the table, not a tlx failure.)
        checks = [counts_check(r, tref, asymmetric_symmetry=k in asymmetric) for (mode, _), r in ((key, v) for key, v in g.items() if key != "tier") if mode != TLC_MODE]
        if tlcn and tref is not tlcn and counts_check(tlcn, tref) is False:
            tlc_differs.append(f"{k} ({tlcn.get('result')} {tlcn.get('distinct')} vs {tref.get('result')} {tref.get('distinct')})")
        ok = "✗" if False in checks else "✓" if any(checks) else "?"
        ref = tref or next((r for r in (c1, i1, cn) if r and ok_result(r)), {})
        row = {"model": k, "tier": g["tier"], "tlc1": t(tlc1), "tlcn": t(tlcn), "i1": t(i1), "c1": t(c1), "cn": t(cn), "ok": ok}
        rows.append(row)
        lines.append(f"| {k} | {g['tier']} | {ref.get('result', '-')} | {ref.get('distinct', '-')} | {cell(tlc1)} | {cell(tlcn)} | {cell(i1)} | "
                     f"{cell(c1)} | {cell(cn)} | {spcell(tlc1, i1)} | {spcell(tlc1, c1)} | {spcell(tlcn, cn)} | {ok} |")
    groups = [(f"tier {tier}", [r for r in rows if r["tier"] == tier]) for tier in sorted({r["tier"] for r in rows})]
    groups += [(name, [r for r in rows if r["tlc1"] and lo <= r["tlc1"] < hi]) for lo, hi, name in TIME_BUCKETS]
    groups.append(("all", rows))
    lines += ["", f"| group | models | interp / TLC 1w | compile 1w speedup | compile {n}w speedup |", "|---|---:|---:|---:|---:|"]
    for name, rs in groups:
        if not rs:
            continue
        gm = lambda a, b: fx(geomean([sp(r[a], r[b]) for r in rs]) if any(sp(r[a], r[b]) for r in rs) else None)
        lines.append(f"| {name} | {len(rs)} | {gm('tlc1', 'i1')} | {gm('tlc1', 'c1')} | {gm('tlcn', 'cn')} |")
    slow_rows = [r for r in rows if r["tlc1"] and r["tlc1"] >= slow_min and r["c1"] and r["tlc1"] / r["c1"] < slow]
    lines += ["", f"Models where tlx -compile 1w is less than {slow:g}× faster than TLC 1w and TLC 1w takes at least {slow_min:g} s: "
              + (", ".join(f"{r['model']} ({r['tlc1'] / r['c1']:.1f}×)" for r in sorted(slow_rows, key=lambda r: r['tlc1'] / r['c1']))
                 or "none") + "."]
    slow_n = [r for r in rows if r["tlcn"] and r["tlcn"] >= slow_min and r["cn"] and r["tlcn"] / r["cn"] < slow]
    lines += ["", f"Models where tlx -compile {n}w is less than {slow:g}× faster than TLC {n}w and TLC {n}w takes at least {slow_min:g} s: "
              + (", ".join(f"{r['model']} ({r['tlcn'] / r['cn']:.1f}×)" for r in sorted(slow_n, key=lambda r: r['tlcn'] / r['cn']))
                 or "none") + "."]
    if tlc_differs:
        lines += ["", f"TLC {n}w's own counts differ from TLC 1w's (complete searches; not compared with tlx): " + ", ".join(tlc_differs) + "."]
    return lines



def scale_counts(rec):
    """'✓' when every tlx run and the TLC runs (or the reference) agree on
    the distinct states and none failed, '✗' when they differ or the repeats
    of one worker count disagree among themselves, '?' without a TLC count."""
    tlx_d = {r.get("distinct") for r in [rec.get("warmup") or {}, *rec.get("tlx", {}).values()] if r.get("result") == "success"}
    tlc_d = {r.get("distinct") for r in rec.get("tlc", {}).values() if r.get("result") == "success"}
    if not tlc_d and rec.get("reference"):
        tlc_d = {rec["reference"]["distinct"]}
    runs = [rec.get("warmup") or {}, *rec.get("tlx", {}).values(), *rec.get("tlc", {}).values()]
    if any(r.get("problems") for r in runs):
        return "✗"  # Repeats of one worker count disagreeing is never a reduction.
    if any(r.get("result") not in (None, "success") + INCOMPLETE for r in runs):
        return "✗"
    if any(r.get("compatibility") is False for r in runs):
        return "-"  # Default-mode reductions may change state counts.
    if len(tlx_d | tlc_d) > 1:
        return "✗"
    return "✓" if tlc_d else "?"



def scale_report(h):
    """Markdown of a scaling file: per model the wall times of tlx and TLC
    per worker count, tlx's speedup over TLC at the same count, and the
    parallel efficiency of each (time at 1 worker / (workers × time); TLC
    relative to its smallest measured count when it has no 1-worker run);
    then the geometric means over the models."""
    ws = sorted({int(w) for m in h["models"].values() for w in [*m.get("tlx", {}), *m.get("tlc", {})]})
    out = ["| model | distinct | | " + " | ".join(f"{w}w" for w in ws) + " | counts |",
           "|---|---:|---|" + "---:|" * len(ws) + ":-:|"]
    t = lambda d, w: (d.get(str(w)) or {}).get("min", (d.get(str(w)) or {}).get("wall_s")) if (d.get(str(w)) or {}).get("result") == "success" else None
    f = lambda x, fmt: "-" if x is None else fmt.format(x)
    speedups, effs, tlc_effs = {}, {}, {}
    for k, rec in h["models"].items():
        tx, tc = rec.get("tlx", {}), rec.get("tlc", {})
        dd = next((r.get("distinct") for r in [*tx.values(), *tc.values()] if r.get("distinct")), None)
        x = {w: t(tx, w) for w in ws}
        c = {w: t(tc, w) for w in ws}
        sp = {w: c[w] / x[w] if c[w] and x[w] else None for w in ws}
        eff = {w: x[1] / (w * x[w]) if x.get(1) and x[w] else None for w in ws}
        cb = min((w for w in ws if c[w]), default=None)
        ceff = {w: cb * c[cb] / (w * c[w]) if cb and c[w] else None for w in ws}
        for w in ws:
            speedups.setdefault(w, []).append(sp[w])
            effs.setdefault(w, []).append(eff[w])
            if cb == 1:
                tlc_effs.setdefault(w, []).append(ceff[w])
        tlc_eff_label = "TLC efficiency" + (f" (vs {cb}w)" if cb and cb != 1 else "")
        out += [f"| {k} | {dd if dd is not None else '-'} | tlx s | " + " | ".join(f(x[w], "{:.2f}") for w in ws) + f" | {scale_counts(rec)} |",
                "| | | TLC s | " + " | ".join(f(c[w], "{:.2f}") for w in ws) + " | |",
                "| | | tlx speedup over TLC | " + " | ".join(f(sp[w], "{:.1f}×") for w in ws) + " | |",
                "| | | tlx efficiency | " + " | ".join(f(None if e is None else 100 * e, "{:.0f}%") for e in eff.values()) + " | |",
                f"| | | {tlc_eff_label} | " + " | ".join(f(None if e is None else 100 * e, "{:.0f}%") for e in ceff.values()) + " | |"]
    gm = lambda vs: geomean([v for v in vs if v]) if any(vs) else None
    out += ["", "| geometric mean | " + " | ".join(f"{w}w" for w in ws) + " |", "|---|" + "---:|" * len(ws),
            "| tlx speedup over TLC | " + " | ".join(f(gm(speedups[w]), "{:.1f}×") for w in ws) + " |",
            "| tlx efficiency | " + " | ".join(f(None if gm(effs[w]) is None else 100 * gm(effs[w]), "{:.0f}%") for w in ws) + " |"]
    if tlc_effs:
        out.append("| TLC efficiency (models with a TLC 1w run) | " + " | ".join(
            f(None if gm(tlc_effs.get(w, [])) is None else 100 * gm(tlc_effs[w]), "{:.0f}%") for w in ws) + " |")
    return out
