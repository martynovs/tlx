"""Saved regression artifacts and incremental campaign report rendering."""
import json
import os
import re
import shutil
from tlxlib import ROOT, no_foreign_home
from .render import spec_files

SPECS = os.path.join(ROOT, "tests", "fuzz", "specs")
VALIDATED = os.path.join(SPECS, "tlc", "validated.json")


def save_regression(src_dir, name, finding, dest=SPECS):
    os.makedirs(os.path.join(dest, "tlc"), exist_ok=True)
    for f in spec_files(src_dir, name):
        shutil.copy2(os.path.join(src_dir, f), os.path.join(dest, f))
    with open(os.path.join(src_dir, "tlc.out")) as f:
        out = f.read()
    with open(os.path.join(dest, "tlc", name + ".out"), "w") as f:
        f.write(normalize(out, src_dir, name))
    with open(os.path.join(dest, name + ".json"), "w") as f:
        json.dump(finding, f, indent=1, sort_keys=True)
        f.write("\n")


def normalize(out, d, name=None):
    """TLC's output without what varies between runs (as spectest's cache).

    `name` is the spec the run was about: its own line names the directory TLC
    read it from, which is the shrinker's scratch directory and not `d` whenever
    a finding is published from a copy of an earlier run."""
    out = out.replace(os.path.realpath(d), "<dir>").replace(d, "<dir>")
    if name:
        out = re.sub(r"(?m)^Parsing file \S*/" + re.escape(name) + r"\.(tla|cfg)$",
                     "Parsing file <dir>/" + name + r".\1", out)
    out = re.sub(r"\d{4}-\d\d-\d\d \d\d:\d\d:\d\d", "<date>", out)
    out = re.sub(r"Finished in \S+( \S+)? at", "Finished in <time> at", out)
    out = re.sub(r"\[pid: \d+\]", "[pid: <pid>]", out)
    out = re.sub(r"(Running .* with \d+ workers?) on \d+ cores .*", r"\1 on <machine>", out)
    out = re.sub(r"\S*/tlc-\d+/", "<tmp>/", out)
    # Every jar SANY read, by its file name (see spectest.normalize): the
    # directory a jar sits in is the recorder's, which module came from which
    # jar is what the line says.
    out = re.sub(r"jar:file:\S*[/\\]([^/\\\s]+\.jar)!", r"jar:file:<\1>!", out)
    return no_foreign_home(out, os.path.join("tests/fuzz/specs/tlc", (name or "?") + ".out"))


def regression_names():
    if not os.path.isdir(SPECS):
        return []
    return sorted(f[:-4] for f in os.listdir(SPECS) if f.endswith(".tla") and os.path.exists(os.path.join(SPECS, f[:-4] + ".cfg")))


def write_report(out_dir, report, cases):
    with open(os.path.join(out_dir, "report.json"), "w") as f:
        json.dump(report, f, indent=1, sort_keys=True, default=str)
    st = report["stats"]
    specs = report["specs"]
    lines = [f"# fuzz_spec seed {report['seed']}", "",
             f"- modes: {', '.join(report['modes'])}; {report.get('secs', 0)} s",
             f"- specs: {len(specs)} generated, {sum(s['status'] == 'pass' for s in specs)} pass, "
             f"{sum(s['status'] == 'fail' for s in specs)} fail, {sum(s['status'] == 'xfail' for s in specs)} known, "
             f"{sum(s['status'] == 'skip' for s in specs)} skipped, "
             f"{sum(s['status'] == 'error' for s in specs)} errors",
             "- TLC results: " + ", ".join(f"{k[4:]} {v}" for k, v in sorted(st.items()) if k.startswith("tlc ")),
             "- other: " + ", ".join(f"{k} {v}" for k, v in sorted(st.items()) if not k.startswith(("tlc ", "status "))),
             f"- cases: {sum(c.status == 'pass' for c in cases)} pass, "
             f"{sum(c.status in ('fail', 'error', 'timeout', 'xpass') for c in cases)} failing, "
             f"{sum(c.status == 'xfail' for c in cases)} known", ""]
    feats = {}
    for s in specs:
        if s["status"] in ("pass", "fail"):
            for w in (s.get("features") or "").split():
                if not w.startswith(("vars=", "actions=")):
                    feats[w] = feats.get(w, 0) + 1
    lines.append("- features of compared specs: " + ", ".join(f"{k} {v}" for k, v in sorted(feats.items())))
    buckets = {}
    for s in specs:
        n = (s.get("tlc") or {}).get("distinct")
        if s["status"] in ("pass", "fail", "xfail"):
            b = "-" if n is None else "0" if n == 0 else "1" if n == 1 else "2-9" if n < 10 else "10-99" if n < 100 else "100+"
            buckets[b] = buckets.get(b, 0) + 1
    lines.append("- TLC distinct states (compared specs): " + ", ".join(f"{k}: {v}" for k, v in sorted(buckets.items())))
    lines += ["", "## Findings", ""]
    for f in report["findings"]:
        lines.append(f"- {f['spec']} {f['mode']}: {f['diffs']} tlc={f['tlc']} tlx={f['tlx']} [{f['features']}]")
        if f.get("shrunk"):
            lines.append(f"  - shrunk: {f['shrunk']['dir']} (reproduces: {f['shrunk']['reproduces']})")
    if not report["findings"]:
        lines.append("none")
    for c in cases:
        if c.id.startswith(("replay/", "expr/")) and c.status not in ("pass", "xfail"):
            lines.append(f"- {c.id}: {c.status} {c.details.get('differs') or c.details.get('message', '')[:300]}")
    with open(os.path.join(out_dir, "report.md"), "w") as f:
        f.write("\n".join(lines) + "\n")
