#!/usr/bin/env python3
"""Feature-combination matrix: generated small specs x tlx settings, against TLC.

Many bugs are interactions of features (VIEW x SYMMETRY, constraints x
fairness, registers x restarts, ...). This script composes small
parameterized base specs (templates) into specs + cfgs that cover all
pairwise (2-way) combinations of the *spec axes* below (a greedy covering
array), all combinations of selected risky axis triples (RISKY_TRIPLES) and
some hand-picked risky rows (RISKY); runs TLC once per spec (results cached
in tests/matrix/expected.json, keyed by a hash of the generated files) and
tlx once per spec and *setting* (tier x workers); and compares the
MUST-level fields.

usage: scripts/matrix.py [--list] [--refresh] [--filter SUBSTR] [--tiers interp,compile,paranoid,prerun]
                         [--workers 1,4] [-j 2] [--tlc-j 2] [-v] [--timeout S]

  --list      print the generated rows (spec name and features) and exit
  --refresh   re-run TLC even for cached specs
  --filter    only specs whose name or feature string contains SUBSTR
              (e.g. M012, prop=live, "effect=regs alias=on")
  --tiers / --workers  the settings to run (default: all)
  -j / --tlc-j         parallel tlx runs / TLC JVMs (default 3 / 2)
  -v          print the tlx output of mismatching runs
Specs are written to target/matrix/specs/NAME/ (NAME.tla, NAME.cfg; TLC's
output in tlc.out); results of the last run go to target/matrix/results.json.
To reproduce a run: cd target/matrix/specs/NAME && tlx check NAME.tla
-workers W [TIER flags] [-simulate|-generate num=4/W -depth 12 -seed 1].
Exit codes: 0 every selected run was judged and matched, 1 a run mismatched or
produced no usable output (no result line, a crash, a kill or a timeout is a
failed run, never a pass: it is reported as FAIL, counted as an infrastructure
failure or inconclusive and named in the refusal line), 2 no verdict (no spec
selected, or no run selected); --list is informational and always exits 0.

Spec axes (AXES; every spec is one row):
  template   mutex (model-value processes acquire / release a lock; cnt
             counts acquisitions, a clock `ticks`) | log (integer counters
             and a bounded sequence of model-value tags)
  mode       bfs | simulate | generate   (simulate/generate use the
             deterministic variant of the template: one initial state, at
             most one successor, so that the result is reproducible)
  reduction  none | view (drops the clock) | symmetry (the model values) | both
  constraint none | constraint | action   (CONSTRAINT / ACTION_CONSTRAINT)
  prop       inv | action | live | post | init   (the checked property:
             INVARIANT, [][A]_v PROPERTY, liveness with WF/SF fairness,
             POSTCONDITION on TLCGet("stats"), implied-init PROPERTY)
  outcome    ok | violation | deadlock | error  (violation: the property of
             `prop` is the failing variant -> invariant / action property /
             liveness (WF instead of SF) / postcondition / implied-init
             failure; deadlock: the action that avoids deadlock is left out;
             error: an evaluation error in Next at a reachable state)
  effect     none | regs | print  (regs: Init sets TLCSet register 1 and an
             always-true invariant counts its evaluations there, printing
             some counts with the state; print: PrintT in an action)
  alias      off | on (an ALIAS with all variables, TLCGet("level") and a
             primed variable)
The axis labels describe the intent: the actual verdict is TLC's (e.g. a
"violation" row under SYMMETRY or a constraint may succeed, an "ok" row of
the deterministic variant may deadlock); every row is compared all the same.
Settings (every spec runs in each applicable one):
  tier       interp | compile | paranoid (-compile, TLX_PARANOID=1) |
             prerun (-compile, TLX_PRERUN=1: forces layout-miss restarts and
             resumed searches)
  workers    1 | 4   (TLC always runs with 1 worker)

Compared by the tiers of the contract (docs/compat.md §0). Exact: result
(an error only as "error": its class and the exit code are not compared),
violated name, distinct / generated / depth, the length of a safety
counterexample, PrintT output (as a sorted multiset). Valid: which
counterexample. If tlx's differs from TLC's (states with their variable
order, action labels), TLC checks that it is a behavior of the spec that
violates the reported property (scripts/tracecheck.py; cached in
tests/matrix/expected.json under "validated"). If TLC's own counterexample
does not pass that check either (VIEW / SYMMETRY: the lasso is over the
reduced graph), it is not compared. Deliberate exclusions (see `compare`),
each a nondeterminism of TLC or of any parallel search:
  - workers > 1 and a non-success result: the counts at the time of the
    stop and the trace are not unique (any of several states of a BFS level
    may be reported first): compared are result, violated, depth and the
    trace length (not checked by TLC). After an error in Next or a deadlock found while
    expanding level L, states of level L + 1 may already exist: the depth is
    not compared either.
  - printed output: workers > 1 with effect=regs (registers are per worker,
    TLC too), with a non-success result (which states were expanded before
    the stop differs), or with VIEW / SYMMETRY (which state of a view class /
    orbit is found first, and so expanded, differs): not compared. After a
    failure or with liveness, the output is compared as a set: TLC evaluates
    Next again to rebuild the counterexample and evaluates ENABLED / <<A>>_v
    for fairness, repeating output.
  - simulation after a failure: TLC's workers go on generating behaviors
    until its main thread stops them, so the number of states generated is
    racy (not compared) and tlx's output must be a subset of TLC's.
  - liveness: the violated properties are not compared; tlx must name at
    least one, and its lasso must violate each (checked by TLC with the
    lasso when the lasso or the names differ). With VIEW / SYMMETRY tlx may
    name none, like TLC, which names the properties its (nondeterministic)
    lasso of concrete states violates, possibly none ("Temporal properties
    were violated."): then the lasso must violate one of the properties.
  - -simulate/-generate with -workers 4 run num=1 per worker (TLC: num is
    per worker), so that both generate 4 behaviors in all.
Every setting applies to every spec. Every template implements every
axis value, and TLC accepts every combination (VIEW / SYMMETRY in
simulation, implied-init properties in simulation (unchecked by TLC),
liveness with SYMMETRY (TLC warns)), so all pairs are generated.

Adding an axis: append (name, [values]) to AXES (the first value is the
default of RISKY rows), implement the values in the templates
(`render_mutex`, `render_log` get the row dict; `cfg_common` writes the
shared cfg parts), and consider RISKY_TRIPLES / RISKY. A new template goes
to RENDER. Settings axes go to TIERS / WORKERS. Appending rows keeps
the names of the existing ones; changing the covering array renames specs,
and changed specs are re-run on TLC automatically (bump CACHE_FORMAT when
the cached fields change).
"""
import argparse, hashlib, itertools, json, os, random, re, shutil, subprocess, sys, time
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor

import contract
from modelcheck import structure
from tlxlib import ROOT, DEFAULT_JOBS, add_profile_args, build_tlx, parity_args, profile_of, run_proc, tlx_env
from tlc import TEMPORAL_VIOLATED, parse, untool, tlc_cmd
import tracecheck

OUT = os.path.join(ROOT, "target", "matrix")
EXPECTED = os.path.join(ROOT, "tests", "matrix", "expected.json")

AXES = [
    ("template", ["mutex", "log"]),
    ("mode", ["bfs", "simulate", "generate"]),
    ("reduction", ["none", "view", "symmetry", "both"]),
    ("constraint", ["none", "constraint", "action"]),
    ("prop", ["inv", "action", "live", "post", "init"]),
    ("outcome", ["ok", "violation", "deadlock", "error"]),
    ("effect", ["none", "regs", "print"]),
    ("alias", ["off", "on"]),
]

# Risky 3-way (and more) combinations, added on top of the pairwise rows.
# Each is a partial row; the other axes take their first value.
# Axis triples covered completely (all value combinations), the other axes
# chosen at random: the interactions behind most past bugs.
RISKY_TRIPLES = [
    ("reduction", "prop", "outcome"),
    ("mode", "constraint", "outcome"),
    ("effect", "reduction", "outcome"),
    ("constraint", "prop", "outcome"),
]

RISKY = [
    dict(reduction="both", prop="live", outcome="violation"),
    dict(reduction="both", prop="live", outcome="ok"),
    dict(reduction="view", prop="live", outcome="violation"),
    dict(reduction="view", prop="live", outcome="ok", template="log"),
    dict(reduction="symmetry", prop="live", outcome="ok"),
    dict(constraint="constraint", prop="live", outcome="ok"),
    dict(constraint="constraint", prop="live", outcome="violation"),
    dict(constraint="action", prop="live", outcome="ok"),
    dict(constraint="action", prop="live", outcome="violation", template="log"),
    dict(mode="simulate", constraint="constraint", outcome="violation"),
    dict(mode="simulate", constraint="action", outcome="ok"),
    dict(mode="generate", constraint="constraint", outcome="ok", template="log"),
    dict(mode="simulate", prop="live", outcome="violation"),
    dict(mode="generate", effect="print", outcome="ok"),
    dict(mode="simulate", effect="regs", outcome="ok", template="log"),
    dict(effect="regs", reduction="view", outcome="ok"),
    dict(effect="regs", reduction="symmetry", outcome="violation"),
    dict(effect="regs", reduction="both", outcome="ok", template="log"),
    dict(effect="print", reduction="both", outcome="ok"),
    dict(effect="print", prop="live", outcome="ok"),
    dict(reduction="view", alias="on", outcome="violation"),
    dict(reduction="symmetry", alias="on", outcome="deadlock"),
    dict(prop="post", outcome="deadlock", effect="regs"),
    dict(prop="post", outcome="violation", reduction="both"),
    dict(prop="action", reduction="symmetry", outcome="violation"),
    dict(prop="init", reduction="view", outcome="violation"),
    dict(outcome="error", reduction="both", alias="on"),
    dict(outcome="error", effect="print", template="log"),
    dict(outcome="deadlock", constraint="action"),
    dict(outcome="deadlock", constraint="constraint", template="log"),
]

TIERS = {
    "interp": (["-interp"], {}),
    "compile": (["-compile"], {}),
    "paranoid": (["-compile"], {"TLX_PARANOID": "1"}),
    "prerun": (["-compile"], {"TLX_PRERUN": "1"}),
}
WORKERS = [1, 4]
SIM_TRACES, SIM_DEPTH = 4, 12
CACHE_FORMAT = "2"


# ---------------------------------------------------------------- covering array

def covering_rows(axes, seed=1):
    """A greedy (AETG-style) pairwise covering array over `axes`.
    Deterministic for a given seed."""
    rnd = random.Random(seed)
    names = [a for a, _ in axes]
    uncovered = set()
    for (i, (a, va)), (j, (b, vb)) in itertools.combinations(enumerate(axes), 2):
        for x in va:
            for y in vb:
                uncovered.add((i, x, j, y))
    rows = []

    def gain(row):
        return sum((i, row[names[i]], j, row[names[j]]) in uncovered for i, j in itertools.combinations(range(len(axes)), 2))

    while uncovered:
        best, best_gain = None, -1
        for _ in range(60):
            # Seed with an uncovered pair, then fill the other axes greedily.
            i, x, j, y = rnd.choice(sorted(uncovered, key=str))
            row = {names[i]: x, names[j]: y}
            order = [k for k in range(len(axes)) if k not in (i, j)]
            rnd.shuffle(order)
            for k in order:
                cands = list(axes[k][1])
                rnd.shuffle(cands)
                scored = []
                for v in cands:
                    r = dict(row)
                    r[names[k]] = v
                    g = sum((min(k, m), r[names[min(k, m)]], max(k, m), r[names[max(k, m)]]) in uncovered
                            for m in range(len(axes)) if names[m] in row)
                    scored.append((g, v))
                row[names[k]] = max(scored, key=lambda t: t[0])[1]
            g = gain(row)
            if g > best_gain:
                best, best_gain = row, g
        rows.append(best)
        for i, j in itertools.combinations(range(len(axes)), 2):
            uncovered.discard((i, best[names[i]], j, best[names[j]]))
    return [{a: r[a] for a in names} for r in rows]


def all_rows():
    rows = covering_rows(AXES)
    seen = {json.dumps(r, sort_keys=True) for r in rows}
    extra = list(RISKY)
    # Full 3-way coverage of the risky axis triples; the other axes vary
    # (seeded, so the rows are stable).
    rnd = random.Random(3)
    for triple in RISKY_TRIPLES:
        values = [dict(AXES)[a] for a in triple]
        for combo in itertools.product(*values):
            partial = dict(zip(triple, combo))
            extra.append({a: partial.get(a, rnd.choice(vals)) for a, vals in AXES})
    for partial in extra:
        r = {a: partial.get(a, vals[0]) for a, vals in AXES}
        if json.dumps(r, sort_keys=True) not in seen:
            seen.add(json.dumps(r, sort_keys=True))
            rows.append(r)
    for n, r in enumerate(rows):
        r["name"] = f'{r["template"][0].upper()}{n:03d}'
    return rows


def features(r):
    return " ".join(f"{a}={r[a]}" for a, _ in AXES)


# ---------------------------------------------------------------- templates

def cfg_common(r, lines, sym, view, constr, aconstr, alias):
    if r["reduction"] in ("symmetry", "both"):
        lines.append(f"SYMMETRY {sym}")
    if r["reduction"] in ("view", "both"):
        lines.append(f"VIEW {view}")
    if r["constraint"] == "constraint":
        lines.append(f"CONSTRAINT {constr}")
    if r["constraint"] == "action":
        lines.append(f"ACTION_CONSTRAINT {aconstr}")
    if r["alias"] == "on":
        lines.append(f"ALIAS {alias}")
    if r["effect"] == "regs":
        lines.append("INVARIANT RegInv")
    bad = r["outcome"] == "violation"
    p = r["prop"]
    if p == "inv":
        lines.append("INVARIANT " + ("InvBad" if bad else "TypeOK"))
    elif p == "action":
        lines.append("PROPERTY " + ("ActBad" if bad else "ActOK"))
    elif p == "live":
        lines.append("PROPERTY Live")
    elif p == "post":
        lines.append("POSTCONDITION " + ("PostBad" if bad else "PostOK"))
    elif p == "init":
        lines.append("PROPERTY " + ("InitBad" if bad else "InitOK"))
    return "\n".join(lines) + "\n"


REGS = '''\
\\* Counts the evaluations (one per distinct state) in register 1 (Init sets
\\* it to 0: TLCGet of an unset register is an error).
RegInv == /\\ TLCSet(1, TLCGet(1) + 1)
          /\\ (TLCGet(1) \\in {{1, 3, 8, 20}} => PrintT(<<"reg", TLCGet(1), {state}>>))
'''

def regs_init(r):
    return " /\\ TLCSet(1, 0)" if r["effect"] == "regs" else ""


POST = '''\
PostOK == TLCGet("stats").distinct >= 1
PostBad == TLCGet("stats").distinct < 3
'''


def render_mutex(r):
    """Processes (model values) acquire / release a lock; cnt counts the
    acquisitions per process (bounded by K, reset when all reach K); ticks
    is a clock modulo 4 that VIEW drops."""
    det = r["mode"] != "bfs"
    live_bad = r["prop"] == "live" and r["outcome"] == "violation"
    print_ = '\n          /\\ PrintT(<<"acq", p, cnt[p], ticks>>)' if r["effect"] == "print" else ""
    error = "\n          /\\ (cnt[p] = 1 /\\ ticks > 1) => 1 \\div 0 = 0" if r["outcome"] == "error" else ""
    pick = "\n          /\\ p = Pick" if det else ""
    reset = r["outcome"] != "deadlock"
    # For liveness, cnt wraps around instead of bounding the acquisitions:
    # then one process can starve under WF (but not under SF).
    wrap = r["prop"] == "live" and reset
    guard = "" if wrap else "\n          /\\ cnt[p] < K"
    inc = "(@ + 1) % (K + 1)" if wrap else "@ + 1"
    fair = (f'\\A p \\in Procs : {"WF" if live_bad else "SF"}_vars(Acq(p)) /\\ WF_vars(Rel(p))'
            + (" /\\ WF_vars(Reset)" if reset else ""))
    tla = f'''---- MODULE {r["name"]} ----
\\* {features(r)}
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS Procs, None, K
VARIABLES owner, cnt, ticks
vars == <<owner, cnt, ticks>>

TypeOK == owner \\in Procs \\cup {{None}} /\\ cnt \\in [Procs -> 0..K] /\\ ticks \\in 0..3
InvBad == \\A p \\in Procs : cnt[p] < 2
Init == owner = None /\\ cnt = [p \\in Procs |-> 0] /\\ ticks = 0{regs_init(r)}
Pick == CHOOSE p \\in Procs : \\A q \\in Procs : cnt[p] <= cnt[q]
Acq(p) == /\\ owner = None{guard}{pick}{print_}
          /\\ owner' = p
          /\\ cnt' = [cnt EXCEPT ![p] = {inc}]
          /\\ ticks' = (ticks + 1) % 4{error}
Rel(p) == owner = p /\\ owner' = None /\\ cnt' = cnt /\\ ticks' = (ticks + 1) % 4
Reset == /\\ owner = None /\\ \\A p \\in Procs : cnt[p] = K
         /\\ cnt' = [p \\in Procs |-> 0] /\\ owner' = owner /\\ ticks' = (ticks + 1) % 4
Next == (\\E p \\in Procs : Acq(p) \\/ Rel(p)){" \\/ Reset" if reset else ""}
Fairness == {fair}
Spec == Init /\\ [][Next]_vars{" /\\ Fairness" if r["prop"] == "live" else ""}
Live == \\A p \\in Procs : []<>(owner = p)
ActOK == [][owner' # owner => (owner = None \\/ owner' = None)]_vars
ActBad == [][owner' # owner => owner = None]_vars
{POST}InitOK == owner = None
InitBad == ticks = 1
Sym == Permutations(Procs)
View == <<owner, cnt>>
Constr == Cardinality({{p \\in Procs : cnt[p] = K}}) < 2
AConstr == owner = None /\\ owner' # None => \\A q \\in Procs : cnt[owner'] <= cnt[q]
Alias == [owner |-> owner, cnt |-> cnt, ticks |-> ticks, lvl |-> TLCGet("level"), next |-> owner']
{REGS.format(state="owner, cnt, ticks")}====
'''
    cfg = cfg_common(r, ["CONSTANTS Procs = {p1, p2, p3} None = None K = 2", "SPECIFICATION Spec"],
                     "Sym", "View", "Constr", "AConstr", "Alias")
    return tla, cfg, ["owner", "cnt", "ticks"]


def render_log(r):
    """Push model-value tags onto a bounded sequence (x counts the pushes,
    bounded by N; Wrap resets it); Pop removes; y is a clock modulo 3 that
    VIEW drops."""
    det = r["mode"] != "bfs"
    live_bad = r["prop"] == "live" and r["outcome"] == "violation"
    print_ = '\n           /\\ PrintT(<<"push", t, x, log>>)' if r["effect"] == "print" else ""
    error = "\n           /\\ (x = 2 => log[3] \\in Tags)" if r["outcome"] == "error" else ""
    pick = "\n           /\\ t = CHOOSE u \\in Tags : TRUE" if det else ""
    wrap = r["outcome"] != "deadlock"
    tla = f'''---- MODULE {r["name"]} ----
\\* {features(r)}
EXTENDS Naturals, Sequences, TLC
CONSTANTS Tags, N
VARIABLES x, y, log
vars == <<x, y, log>>

TypeOK == x \\in 0..N /\\ y \\in 0..2 /\\ log \\in Seq(Tags) /\\ Len(log) <= 2
InvBad == Len(log) < 2
Init == x = 0 /\\ y = 0 /\\ log = <<>>{regs_init(r)}
Push(t) == /\\ x < N{pick}{print_}
           /\\ x' = x + 1
           /\\ log' = IF Len(log) < 2 THEN Append(log, t) ELSE Append(Tail(log), t)
           /\\ y' = (y + 1) % 3{error}
Pop == {"FALSE /\\ " if det else ""}x > 0 /\\ x < N /\\ log # <<>> /\\ x' = x - 1 /\\ log' = Tail(log) /\\ y' = (y + 1) % 3
Wrap == x = N /\\ x' = 0 /\\ log' = <<>> /\\ y' = y
Next == (\\E t \\in Tags : Push(t)) \\/ Pop{" \\/ Wrap" if wrap else ""}
Fairness == WF_vars(Next)
Spec == Init /\\ [][Next]_vars{" /\\ Fairness" if r["prop"] == "live" else ""}
Live == {"[]<>(x = N)" if live_bad else "[]<>(y = 0) /\\ []<>(x # N)"}
ActOK == [][x' # x]_vars
ActBad == [][x' >= x]_vars
{POST}InitOK == x = 0 /\\ log = <<>>
InitBad == log # <<>>
Sym == Permutations(Tags)
View == <<x, log>>
Constr == x + y < N + 2
AConstr == ~(x' < x /\\ y = 1)
Alias == [x |-> x, y |-> y, log |-> log, len |-> Len(log), lvl |-> TLCGet("level"), nx |-> x']
{REGS.format(state="x, y, log")}====
'''
    cfg = cfg_common(r, ["CONSTANTS Tags = {a, b} N = 3", "SPECIFICATION Spec"],
                     "Sym", "View", "Constr", "AConstr", "Alias")
    return tla, cfg, ["x", "y", "log"]


RENDER = {"mutex": render_mutex, "log": render_log}


def sim_flags(r, workers):
    if r["mode"] == "bfs":
        return []
    num = SIM_TRACES // workers
    return [f'-{r["mode"]}', f"num={num}", "-depth", str(SIM_DEPTH), "-seed", "1"]


def write_spec(r):
    tla, cfg, vars_ = RENDER[r["template"]](r)
    d = os.path.join(OUT, "specs", r["name"])
    os.makedirs(d, exist_ok=True)
    for ext, text in ((".tla", tla), (".cfg", cfg)):
        p = os.path.join(d, r["name"] + ext)
        current = Path(p).read_text() if os.path.exists(p) else None
        if current != text:
            Path(p).write_text(text)
    r["dir"], r["vars"] = d, vars_
    # (CACHE_FORMAT: bump when the cached fields change.)
    r["hash"] = hashlib.sha1((CACHE_FORMAT + tla + cfg + " ".join(sim_flags(r, 1))).encode()).hexdigest()[:16]


# ---------------------------------------------------------------- output parsing

def parse_trace(out):
    """The counterexample: (states, loop), states a list of (label, {var: value
    text}), loop None, "stutter" or the 1-based index of the loop target.
    The driver reads the plain standard output of a run (tracecheck reads a
    -tool output: the same states, numbered step labels and message codes)."""
    states, loop = [], None
    blocks = re.split(r"\n(?=State \d+: |Back to state \d+: )", "\n" + out)
    for b in blocks:
        b = b.strip("\n")
        head, _, body = b.partition("\n")
        if (target := structure.loop_target(head)) is not None:
            loop = target
        elif m := re.match(r"State \d+: (.*)", head):
            # The state ends at the first empty line.
            states.append((m.group(1).strip(), parse_state(body.split("\n\n", 1)[0])))
    if not states and (m := re.search(r"is violated by the initial state:\n(.*?)(?:\n\n|\Z)", out, re.S)):
        states.append(("Initial predicate", parse_state(m.group(1))))
    return states, loop


def parse_state(body):
    return [list(p) for p in structure.state(body)]


def summarize(out, wall, rc):
    out = untool(out)
    r = parse(out, wall, rc)
    if m := TEMPORAL_VIOLATED.search(out):
        r["violated"] = m.group(2) or (m.group(3) or "").strip() or None
    if m := re.search(r"Error: Action property (\S+) is violated", out):
        r["violated"] = m.group(1)
    states, loop = parse_trace(out)
    r["trace"] = states
    r["loop"] = loop
    r["prints"] = sorted(line for line in out.splitlines() if line.startswith('<<"'))
    if r.get("result") == "error":
        r["error"] = r.get("error", "")[:300]
    return r


# ---------------------------------------------------------------- runners

def run_tlc_raw(cwd, name, args, timeout):
    meta = os.path.join(ROOT, "target", "tlc-states", f"matrix-{name}-{os.getpid()}")
    cmd = tlc_cmd(name + ".tla", args, state_dir=meta)
    try:
        p = run_proc(cmd, timeout, cwd=cwd, raise_timeout=False)
        return p.stdout + p.stderr, p.wall, p.returncode
    finally:
        shutil.rmtree(meta, ignore_errors=True)


def run_tlc(r, timeout):
    out, wall, rc = run_tlc_raw(r["dir"], r["name"], ["-config", r["name"] + ".cfg", "-workers", "1", *sim_flags(r, 1)], timeout)
    with open(os.path.join(r["dir"], "tlc.out"), "w") as output:
        output.write(out)
    s = summarize(out, wall, rc)
    if rc == -9 and wall >= timeout:
        s = {"result": "timeout"}
    s.pop("wall_s", None)
    s.pop("left", None)
    s["hash"] = r["hash"]
    return s


def run_tlx(binary, r, tier, workers, timeout):
    flags, env = TIERS[tier]
    cmd = [binary, "check", r["name"] + ".tla", "-config", r["name"] + ".cfg", "-workers", str(workers),
           *sim_flags(r, workers), *parity_args(binary, flags)]
    t0 = time.time()
    try:
        p = run_proc(cmd, timeout, cwd=r["dir"], env=tlx_env(extra=env))
    except subprocess.TimeoutExpired:
        return {"result": "timeout"}, ""
    out = p.stdout + p.stderr
    return summarize(out, time.time() - t0, p.returncode), out


# ---------------------------------------------------------------- comparison

KEYS = ("result", "violated", "exit", "distinct", "generated", "depth")


def validate(r, want, got, cache, timeout):
    """TLC's check of tlx's counterexample (tracecheck.check_against, with
    TLC's as the control): (verdict, detail)."""
    kind = want["result"]

    def names(res):
        if kind == "liveness failure":
            return tracecheck.property_names(res.get("violated")) or None
        return [res.get("violated")]

    def ce(res):
        return [[tuple(p) for p in pairs] for _, pairs in res.get("trace") or []], res.get("loop")

    cfg = tracecheck.read(os.path.join(r["dir"], r["name"] + ".cfg"))
    return tracecheck.check_against(r["dir"], r["name"], cfg, kind, ce(got), names(got), ce(want), names(want),
                                    variables=r["vars"], simulation=r["mode"] != "bfs", cache=cache, timeout=timeout)


def compare(r, want, got, workers):
    """Mismatch descriptions (empty: match); a 'validate' entry (tlx's
    counterexample differs from TLC's) is resolved by the caller with
    `validate`."""
    if want.get("result") == "timeout":
        return ["TLC oracle timed out"]
    diffs = []
    sim = r["mode"] != "bfs"
    success = want.get("result") == "success"
    live = want.get("result") == "liveness failure"
    exact = workers == 1 or success or sim
    keys = ["result"]
    if not live:
        keys.append("violated")
    elif not got.get("violated") and r["reduction"] == "none" and got.get("result") == "liveness failure":
        # (With VIEW / SYMMETRY TLC names the violated properties by
        # evaluating them on its lasso of concrete states, which need not
        # violate any: "Temporal properties were violated.")
        diffs.append("violated: tlx names no violated property")
    if sim:
        # (After an error TLC's workers go on until the main thread stops
        # them: the count is racy.)
        keys += ["generated"] if success else []
    else:
        keys += ["distinct", "generated"] if exact else []
        # (Several workers that stop at an error in Next or a deadlock while
        # expanding level L may already have found states of level L + 1.)
        if exact or want.get("result") not in ("error", "deadlock"):
            keys.append("depth")
    for k in keys:
        if want.get(k) != got.get(k):
            diffs.append(f"{k}: tlc={want.get(k)} tlx={got.get(k)}")
    if want.get("result") == got.get("result") and want.get("result") in tracecheck.KINDS:
        wt, gt = want.get("trace") or [], got.get("trace") or []
        wt = [tuple(map(json.dumps, s)) for s in wt]
        gt = [tuple(map(json.dumps, s)) for s in gt]
        # (Safety counterexamples are shortest: BFS. A simulation's are not.)
        if not live and not sim and len(wt) != len(gt):
            diffs.append(f"trace length: tlc={len(wt)} tlx={len(gt)}")
        elif not diffs and (live or workers == 1 or sim):
            if (wt, want.get("loop"), want.get("violated")) != (gt, got.get("loop"), got.get("violated")):
                diffs.append("validate")
    # (Several workers: registers are per worker; after a stop, which states
    # were expanded differs; with SYMMETRY / VIEW, which state of an orbit /
    # a view is found first, and so expanded, differs.)
    compare_prints = (r["effect"] != "none" and not (workers > 1 and r["effect"] == "regs")
                      and not (workers > 1 and not success and not sim)
                      and not (workers > 1 and not sim and r["reduction"] != "none"))
    if compare_prints:
        wp, gp = want.get("prints") or [], got.get("prints") or []
        if not success or r["prop"] == "live":
            # TLC evaluates Next again to rebuild a counterexample and to
            # evaluate ENABLED / <<A>>_v for fairness, repeating the output.
            wp, gp = sorted(set(wp)), sorted(set(gp))
        # (After an error TLC's simulation workers go on generating
        # behaviors until the main thread stops them: more output.)
        racy = sim and not success
        if (not set(gp) <= set(wp)) if racy else wp != gp:
            diffs.append(f"prints differ (tlc {len(wp)}, tlx {len(gp)}): "
                         f"tlc-only {sorted(set(wp) - set(gp))[:3]} tlx-only {sorted(set(gp) - set(wp))[:3]}")
    return diffs


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--refresh", action="store_true")
    ap.add_argument("--filter", default="")
    ap.add_argument("--tiers", default=",".join(TIERS))
    ap.add_argument("--workers", default=",".join(map(str, WORKERS)))
    ap.add_argument("-j", "--jobs", dest="j", type=int, default=DEFAULT_JOBS)
    ap.add_argument("--tlc-j", type=int, default=2)
    ap.add_argument("--timeout", type=int, default=120)
    ap.add_argument("-v", action="store_true")
    add_profile_args(ap)
    a = ap.parse_args()
    rows = [r for r in all_rows() if a.filter in r["name"] or a.filter in features(r)]
    if a.list:
        for r in rows:
            print(r["name"], features(r))
        print(f"{len(rows)} specs")
        return
    if not rows:
        print(f"matrix: no spec selected (--filter {a.filter!r} matched none of {len(all_rows())} rows)", file=sys.stderr)
        sys.exit(2)
    for r in rows:
        write_spec(r)
    if os.path.exists(EXPECTED):
        with open(EXPECTED) as source:
            expected = json.load(source)
    else:
        expected = {}
    todo = [r for r in rows if a.refresh or expected.get(r["name"], {}).get("result") == "timeout"
            or expected.get(r["name"], {}).get("hash") != r["hash"]]
    if todo:
        print(f"running TLC on {len(todo)} specs", flush=True)
        with ThreadPoolExecutor(a.tlc_j) as ex:
            for r, s in zip(todo, ex.map(lambda r: run_tlc(r, a.timeout), todo)):
                expected[r["name"]] = s
                print(f'  TLC {r["name"]}: {s.get("result")} {s.get("violated") or ""}', flush=True)
        os.makedirs(os.path.dirname(EXPECTED), exist_ok=True)
        with open(EXPECTED, "w") as output:
            json.dump(expected, output, indent=1, sort_keys=True)
    # Drop cache entries of specs that are no longer generated (full runs only).
    if not a.filter:
        names = {r["name"] for r in rows}
        for k in [k for k in expected if k not in names and k != "validated"]:
            del expected[k]
    # TLC's checks of tlx's counterexamples (scripts/tracecheck.py; "lasso":
    # the format before it).
    expected.pop("lasso", None)
    validated = expected.setdefault("validated", {})
    notes = {}

    with build_tlx(profile=profile_of(a), prefix="tlx-matrix-") as binary:
        check_matrix(a, rows, expected, validated, notes, binary)


def check_matrix(a, rows, expected, validated, notes, binary):
    tiers = [t for t in a.tiers.split(",") if t]
    workers = [int(w) for w in a.workers.split(",") if w]
    jobs = [(r, t, w) for r in rows for t in tiers for w in workers]
    if not jobs:
        print(f"matrix: no run selected ({len(rows)} specs, --tiers {a.tiers!r}, --workers {a.workers!r})", file=sys.stderr)
        sys.exit(2)
    print(f"{len(rows)} specs, {len(jobs)} tlx runs (0 inapplicable: {{}})", flush=True)

    def one(job):
        r, t, w = job
        got, out = run_tlx(binary, r, t, w, a.timeout)
        return job, got, out

    observations = contract.Observations(selected=len(jobs), eligible=len(jobs))
    results, fails = {}, []
    with ThreadPoolExecutor(a.j) as ex:
        for (r, t, w), got, out in ex.map(one, jobs):
            observations.count_result(got)
            want = expected[r["name"]]
            diffs = compare(r, want, got, w)
            if "validate" in diffs:
                diffs.remove("validate")
                verdict, detail = validate(r, want, got, validated, a.timeout)
                notes[verdict] = notes.get(verdict, 0) + 1
                if verdict in ("invalid", "error"):
                    diffs.append(f"tlx counterexample differs from TLC's and is {verdict}: {detail}")
                elif verdict == "unchecked" and a.v:
                    print(f'NOTE {r["name"]}/{t}/w{w}: counterexample differs, unchecked: {detail}', flush=True)
            key = f'{r["name"]}/{t}/w{w}'
            results[key] = {"ok": not diffs, "known": None, "diffs": diffs, "features": features(r),
                            "tlx": {k: got.get(k) for k in KEYS}}
            if diffs:
                fails.append(key)
                print(f'FAIL {key:22} {features(r)}\n     ' + "\n     ".join(diffs), flush=True)
                if a.v:
                    print("     " + out[-1500:].replace("\n", "\n     "))
    with open(EXPECTED, "w") as output:
        json.dump(expected, output, indent=1, sort_keys=True)
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, "results.json"), "w") as output:
        json.dump(results, output, indent=1)
    print(f"\n{len(results) - len(fails)}/{len(results)} runs passed, 0 known issues, "
          f"{len(fails)} failed ({len(rows)} specs); counterexamples differing from TLC's: "
          + (", ".join(f"{v} {k}" for k, v in sorted(notes.items())) or "none"))
    print(f"observations: {observations.summary()}")
    refusal = observations.refusal()
    if refusal:
        print(f"matrix: {refusal}")
    sys.exit(1 if fails else 2 if refusal else 0)


if __name__ == "__main__":
    main()
