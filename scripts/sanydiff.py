#!/usr/bin/env python3
"""Compare tlx's parse/semantic checks with SANY's on .tla files.

Runs SANY (tla2sany.SANY) and tlx (`tlx-sema` example `check`, i.e. what
`Program::load` reports) on every file and prints the files where one
accepts and the other rejects. SANY results are cached in
corpus/out/sany.json (keyed by path and mtime; refresh with --refresh).

usage: scripts/sanydiff.py [-j N] [--refresh] [FILE_OR_DIR...]

  FILE_OR_DIR...  .tla files or directories (default: every .tla under
                  third_party/Examples/specifications)
  -j N            parallel jobs (default 3)
  --refresh       rerun SANY instead of using the cache
Environment: TLX_JAVA (see scripts/tlc.py).

Exit codes: 0 no difference, 1 differences, 2 no verdict (nothing selected, or
a file where SANY or the tlx front end produced no verdict at all - a crash or
a missing output is an infrastructure failure, never a difference and never an
acceptance).
"""
import argparse, json, os, re, subprocess, sys
from concurrent.futures import ThreadPoolExecutor

import contract
from tlxlib import ROOT, TP, add_profile_args, include_args, profile_dir, profile_of, run_proc
from tlc import CLASSPATH, DEFAULT_JOBS, JAVA

CACHE = os.path.join(ROOT, "corpus", "out", "sany.json")


def sany(path):
    """SANY's verdict on one file: {"ok": bool|None, "msg": str}. `ok` is None
    when SANY itself produced no verdict (it timed out or could not run): no
    comparison, and no caching, so the next run tries again."""
    try:
        p = run_proc([JAVA, "-cp", CLASSPATH, "tla2sany.SANY", os.path.basename(path)],
                     cwd=os.path.dirname(path), timeout=300)
    except Exception as e:  # noqa: BLE001
        return {"ok": None, "msg": f"SANY produced no verdict: {type(e).__name__}: {e}"}
    out = p.stdout + p.stderr
    bad = p.returncode != 0 or "*** Errors:" in out or "Could not parse" in out or "Fatal errors" in out
    msg = ""
    if bad:
        keep = [l.strip() for l in out.splitlines() if l.strip() and not re.match(
            r"(Parsing file|Semantic processing|SANY finished|\*\*\*|Linting|Fatal errors|Semantic errors|Parsing or semantic)|.*Version 2", l.strip())]
        msg = " | ".join(keep)[:300]
    return {"ok": not bad, "msg": msg}


def collect(args):
    """The .tla files to compare, and the arguments that name nothing."""
    files, missing = [], []
    for a in args or [os.path.join(TP, "Examples", "specifications")]:
        if os.path.isdir(a):
            for d, _, fs in os.walk(a):
                files += [os.path.join(d, f) for f in fs if f.endswith(".tla") and "_TTrace_" not in f]
        elif os.path.isfile(a):
            files.append(os.path.abspath(a))
        else:
            missing.append(a)
    return sorted(files), missing


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("paths", nargs="*", metavar="FILE_OR_DIR")
    ap.add_argument("-j", "--jobs", dest="j", type=int, default=DEFAULT_JOBS)
    ap.add_argument("--refresh", action="store_true")
    add_profile_args(ap)
    a = ap.parse_args()
    jobs, refresh = a.j, a.refresh
    files, missing = collect(a.paths)
    if missing:
        print(f"sanydiff: not a file or directory: {', '.join(missing)}", file=sys.stderr)
        sys.exit(2)
    if not files:
        print(f"sanydiff: no .tla file selected ({' '.join(a.paths) or 'the default directory'})", file=sys.stderr)
        sys.exit(2)
    cache = {}
    if os.path.exists(CACHE) and not refresh:
        with open(CACHE) as stream:
            cache = json.load(stream)

    def cached(f):
        e = cache.get(f)
        return e is not None and e.get("mtime") == os.path.getmtime(f) and e.get("ok") is not None

    todo = [f for f in files if not cached(f)]
    with ThreadPoolExecutor(jobs) as ex:
        fresh = dict(zip(todo, ex.map(sany, todo)))
    for f, r in fresh.items():
        if r["ok"] is not None:
            r["mtime"] = os.path.getmtime(f)
            cache[f] = r
    os.makedirs(os.path.dirname(CACHE), exist_ok=True)
    with open(CACHE, "w") as out:
        json.dump(cache, out, indent=1, sort_keys=True)

    profile = profile_of(a)
    subprocess.run(["cargo", "build", "-q", "--profile", profile, "-p", "tlx-sema", "--example", "check"], cwd=ROOT, check=True)
    exe = os.path.join(ROOT, "target", profile_dir(profile), "examples", "check")
    inc = include_args()
    tlx = {}

    def run(chunk):
        p = run_proc([exe, *inc, *chunk], timeout=600)
        if p.returncode != 0:
            print(f"tlx check exited {p.returncode} on {len(chunk)} file(s)", file=sys.stderr)
        return p.stdout

    chunks = [files[i::jobs] for i in range(jobs)]
    with ThreadPoolExecutor(jobs) as ex:
        for out in ex.map(run, chunks):
            for line in out.splitlines():
                m = re.match(r"(OK|ERR) (.*?\.tla)(?:: (.*))?$", line)
                if m:
                    tlx[m.group(2)] = (m.group(1) == "OK", m.group(3) or "")
    observations = contract.Observations(selected=len(files), eligible=len(files))
    diffs = 0
    for f in files:
        s = cache.get(f) or fresh.get(f) or {}
        t = tlx.get(f)
        if s.get("ok") is None or t is None:
            observations.count(contract.INFRA_FAILED)
            why = (f"SANY produced no verdict ({s.get('msg', '')})" if s.get("ok") is None
                   else "tlx printed no OK/ERR line for it (crash, killed, or not reached)")
            print(f"{os.path.relpath(f, ROOT)}\n  no verdict: {why}")
            continue
        observations.count()
        if s["ok"] != t[0]:
            diffs += 1
            print(f"{os.path.relpath(f, ROOT)}\n  SANY: {'OK' if s['ok'] else s['msg']}\n  tlx:  {'OK' if t[0] else t[1]}")
    nbad = sum(cache.get(f, {}).get("ok") is False for f in files)
    print(f"{len(files)} files ({nbad} rejected by SANY), {diffs} differences")
    print(f"observations: {observations.summary()}")
    refusal = observations.refusal()
    if refusal:
        print(f"sanydiff: {refusal}")
    sys.exit(1 if diffs else 2 if refusal else 0)


if __name__ == "__main__":
    main()
