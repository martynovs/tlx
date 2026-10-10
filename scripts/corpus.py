#!/usr/bin/env python3
"""Corpus of TLC models: the tlaplus/Examples manifests (third_party/Examples) and
the external repositories of corpus/external.json (fetched by scripts/fetch-external.py).

usage:
  scripts/corpus.py list [--max-runtime SECS]          # list the models (exhaustive search, simulate, generate)
  scripts/corpus.py tlc  [--max-runtime SECS] [-j N] [--only-failed]
                                                       # run TLC on each, write/merge corpus/out/tlc.json
  scripts/corpus.py tlc --min-runtime 61 --max-runtime 700 --name tlc-large --tlc-workers 4
                                                       # the large tier (corpus/out/tlc-large.json)
  scripts/corpus.py tlc --tier huge --tlc-workers 16   # manifest runtime above 700 s (corpus/out/tlc-huge.json; for the cloud)
  scripts/corpus.py list --source external             # the external models (ids ext/<repo>/<model>), tiers small and medium
  scripts/corpus.py tlc --source external --tier small # their TLC oracle: corpus/out/tlc-ext.json

  --source S       examples (default) or external
  --tier T[,T...]  examples: large (manifest runtime 61-700 s, oracle tlc-large) or huge (above
                   700 s, oracle tlc-huge); external: tiers of corpus/external.json
                   (small, medium, large, huge; default small,medium; oracle tlc-ext, or
                   tlc-ext-<tier> for the others). The default selection of both sources
                   never includes the large and huge models.
  --repos R,...    external: only these repositories
  --max-runtime S  only models whose manifest runtime is at most S seconds (default 10)
  --min-runtime S  ... and at least S seconds (default 0)
  -j N             TLC runs in parallel (default 3)
  --name NAME      oracle file corpus/out/NAME.json (default tlc, or see --tier)
  --tlc-heap SIZE  TLC's JVM -Xmx, e.g. 12g (default: the JVM's, a quarter of the memory per JVM)
  --tlc-workers N  TLC -workers (default 1; an external model's `oracle_workers` wins: models that
                   may end in a violation have 1, for deterministic counts)
  --timeout S      per-model timeout in seconds (default 600)
  --only-failed    rerun only the models whose TLC result was 'error'
Environment: TLX_JAVA (see scripts/tlc.py).
"""
import argparse, glob, json, os, re, sys, threading
from concurrent.futures import ThreadPoolExecutor

from tlxlib import ROOT, json_update
from oracle import provenance, issue
# Real path: inside a worktree third_party is a symlink to the main checkout, and
# recorded spec paths must stay valid after the worktree is removed.
EX = os.path.realpath(os.path.join(ROOT, "third_party", "Examples"))
OUT = os.path.join(ROOT, "corpus", "out")
EXTERNAL_MANIFEST = os.path.join(ROOT, "corpus", "external.json")
EXTERNAL = os.path.join(os.path.realpath(os.path.join(ROOT, "third_party")), "external")
EXTERNAL_TIERS = ("small", "medium", "large", "huge")
# The huge tier of the Examples: models whose manifest runtime is above 700 s (the
# models with an unknown runtime are left out: secs() gives them 10**9).
HUGE_MIN_RUNTIME = 701
HUGE_MAX_RUNTIME = 10**8
# Behaviors generated for models in "generate" mode.
GENERATE_TRACES = 100


def secs(rt):
    if not rt or rt == "unknown":
        return 10**9
    h, m, s = (int(x) for x in rt.split(":"))
    return h * 3600 + m * 60 + s


def external_models(tiers=("small", "medium"), repos=()):
    """The models of corpus/external.json of the given tiers, in the form of
    `models` (ids `ext/<repo>/<name>`, paths below third_party/external)."""
    with open(EXTERNAL_MANIFEST) as source:
        manifest = json.load(source)
    res = []
    for m in manifest["models"]:
        if m["tier"] not in tiers or (repos and m["repo"] not in repos):
            continue
        base = os.path.join(EXTERNAL, m["repo"])
        res.append({
            "id": f'ext/{m["repo"]}/{m["name"]}',
            "tla": os.path.join(base, m["tla"]),
            "cfg": os.path.join(base, m["cfg"]),
            "runtime": m.get("runtime", "unknown"),
            "result": m.get("result"),
            "distinct": None,
            "total": None,
            "features": m.get("features", []),
            "flags": m.get("flags", []),
            "tier": m["tier"],
            **({"oracle_workers": m["oracle_workers"]} if m.get("oracle_workers") else {}),
            **({"asymmetric_symmetry": True} if m.get("asymmetric_symmetry") else {}),
            **({"scratch": True} if m.get("scratch") else {}),
        })
    return res


def select(a):
    """The models selected by the command line, and the name of their oracle file."""
    tiers = [t for t in a.tier.split(",") if t]
    if a.source == "external":
        tiers = tiers or ["small", "medium"]
        bad = [t for t in tiers if t not in EXTERNAL_TIERS]
        if bad:
            sys.exit(f"unknown external tier(s) {bad}; known: {', '.join(EXTERNAL_TIERS)}")
        name = a.name or ("tlc-ext" if set(tiers) <= {"small", "medium"} else "tlc-ext-" + "-".join(tiers))
        return external_models(tiers, [r for r in a.repos.split(",") if r]), name
    if tiers not in ([], ["large"], ["huge"]):
        sys.exit("--tier of the examples: large or huge (the default tier is chosen with --max-runtime)")
    if tiers == ["large"]:
        a.min_runtime, a.max_runtime = max(a.min_runtime, 61), 700
    elif tiers == ["huge"]:
        a.min_runtime, a.max_runtime = max(a.min_runtime, HUGE_MIN_RUNTIME), HUGE_MAX_RUNTIME
    return models(a.max_runtime, a.min_runtime), a.name or ("tlc" if not tiers else f"tlc-{tiers[0]}")


def models(max_runtime, min_runtime=0):
    res = []
    for mf in sorted(glob.glob(os.path.join(EX, "specifications", "*", "manifest.json"))):
        with open(mf) as stream:
            d = json.load(stream)
        for mod in d["modules"]:
            for m in mod.get("models", []):
                mode = m.get("mode")
                runtime = m.get("runtime")
                # Run in a copy of the spec's directory (the spec writes files).
                scratch = False
                if mode == "exhaustive search":
                    flags = []
                elif isinstance(mode, dict) and "simulate" in mode:
                    # Random simulation: only the kind of result is comparable.
                    flags = ["-simulate", f'num={mode["simulate"]["traceCount"]}']
                elif mode == "generate":
                    # Unbounded in the manifest (runtime "unknown"): a fixed
                    # number of behaviors. The generation models collect
                    # statistics with CSVWrite.
                    flags = ["-generate", f"num={GENERATE_TRACES}"]
                    runtime, scratch = "00:00:05", True
                else:
                    continue
                if not min_runtime <= secs(runtime) <= max_runtime:
                    continue
                res.append({
                    "id": m["path"].removeprefix("specifications/").removesuffix(".cfg"),
                    "tla": os.path.join(EX, mod["path"]),
                    "cfg": os.path.join(EX, m["path"]),
                    "runtime": runtime,
                    "result": m.get("result"),
                    "distinct": m.get("distinctStates"),
                    "total": m.get("totalStates"),
                    "features": mod.get("features", []) + m.get("features", []),
                    "flags": flags,
                    **({"scratch": True} if scratch else {}),
                })
    return res


def main():
    import tlc
    from tlc import DEFAULT_JOBS, run_tlc

    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cmd", choices=["list", "tlc"])
    ap.add_argument("--max-runtime", type=int, default=10)
    ap.add_argument("-j", "--jobs", dest="j", type=int, default=DEFAULT_JOBS)
    ap.add_argument("--min-runtime", type=int, default=0)
    ap.add_argument("--source", choices=["examples", "external"], default="examples")
    ap.add_argument("--tier", default="", help="see above")
    ap.add_argument("--repos", default="", help="external: only these repositories (comma-separated)")
    ap.add_argument("--name", default="", help="oracle file corpus/out/<name>.json (default: tlc, or see --tier)")
    ap.add_argument("--tlc-workers", type=int, default=1)
    ap.add_argument("--tlc-heap", default="", metavar="SIZE",
                    help="TLC's JVM -Xmx (and a quarter of it as -XX:MaxDirectMemorySize), e.g. 12g")
    ap.add_argument("--timeout", type=int, default=600)
    ap.add_argument("--only-failed", action="store_true", help="rerun only models whose TLC result was 'error'")
    a = ap.parse_args()
    ms, name = select(a)
    if a.tlc_heap:
        # (Several TLC JVMs at a time: the JVM's default heap is a quarter of
        # the physical memory each, and together they can exhaust it.)
        m = re.fullmatch(r"(\d+)([gGmM])", a.tlc_heap)
        if not m:
            sys.exit(f"--tlc-heap expects a size like 12g or 4096m, got {a.tlc_heap!r}")
        tlc.JVM_OPTS += [f"-Xmx{a.tlc_heap}", f"-XX:MaxDirectMemorySize={max(1, int(m.group(1)) // 4)}{m.group(2)}"]
    if a.cmd == "list":
        for m in ms:
            print(f'{m["id"]:60} {m["runtime"]:>9} {m["result"] or "-":16} {m["distinct"]}')
        print(len(ms), "models", file=sys.stderr)
        return
    path = os.path.join(OUT, f"{name}.json")
    if os.path.exists(path):
        with open(path) as source:
            prev = json.load(source)
    else:
        prev = {}
    lock = threading.Lock()

    def one(m):
        workers = m.get("oracle_workers") or a.tlc_workers
        try:
            # (A model that may end in a violation runs with the workers the
            # manifest asks for: one, for TLC's counts and depth to be deterministic.)
            r = run_tlc(m["tla"], ["-config", m["cfg"], "-workers", str(workers), *m["flags"]], timeout=a.timeout, features=m["features"],
                        scratch=m.get("scratch", False))
        except Exception as e:  # timeout etc.
            r = {"result": "collection error", "error": str(e)}
        try:
            evidence = provenance(m, workers, r)
        except OSError as error:
            evidence = {}
            r = {"result": "collection error", "error": str(error)}
        print(f'{m["id"]:60} {r.get("result"):16} {r.get("distinct")} ({r.get("wall_s")}s)', flush=True)
        # Saved after every model, so an interrupted run keeps its results.
        with lock:
            prev[m["id"]] = {**m, "tlc": r, "provenance": evidence}
            json_update(path, lambda data: data.update({m["id"]: prev[m["id"]]}))

    if a.only_failed:
        ms = [m for m in ms if issue({**prev.get(m["id"], {}), **m}) or not prev.get(m["id"], {}).get("provenance")]
    with ThreadPoolExecutor(a.j) as ex:
        list(ex.map(one, ms))
    bad = [m["id"] for m in ms if issue(prev[m["id"]])]
    print(f"coverage: selected={len(ms)}, complete={len(ms) - len(bad)}, incomplete={len(bad)}")
    if bad:
        sys.exit(1)


if __name__ == "__main__":
    main()
