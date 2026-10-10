#!/usr/bin/env python3
"""Prove a refactor left the generated IR and model.c byte-identical.

Builds (or reuses) the release binary of BASE_REV and CANDIDATE_REV, runs a
fixed list of small specs through the compiled tier with a fresh native-code
cache and the knobs that dump the typed IR and the generated C, normalizes the
volatile parts of the output, and compares the IR dump, model.c, the search
output and the exit code per spec. A behavior-preserving cleanup batch reports
this receipt instead of a cloud HOT-* timing A/B (docs/testing.md, "Helper drivers: exit codes").

usage: scripts/ir_receipt.py BASE_REV CANDIDATE_REV [--specs NAME...] --out FILE.json

Default specs (tests/specs/NAME.tla + NAME.cfg), chosen to cover the compiled
tiers with short runs (the JSON records these reasons):

  B6      invariant violation (exit 12): one INVARIANT and one action property
  L2      a `<>` liveness property: the stutter-free search path
  C3      a state-level PROPERTY (checked on states, not on actions)
  L4      nested `[]<>` and a leads-to `~>`
  L5      WF fairness plus `<>done`
  LC6     SF fairness and the `[]<>` / `<>[]` implication
  EK6     an action property that cannot be evaluated (exit 75)
  CG4     encoded-set coercion with the interpreted fallback: INVARIANT + two action properties
  CMP73A  0-indexed arrays and three action properties; the largest IR dump of the list

Both revisions run the same spec files (the invoking checkout's tests/specs,
copied into a private scratch directory) and the same knobs
(TLX_JIT/NATIVE_SYNC/PRERUN_REUSE/IR_DUMP/IR_DUMP_RAW/IR_VERIFY_HARD with a
fresh TLX_CACHE per run), so a receipt compares the two revisions' generated
code alone.

Exit codes: 0 identical, 1 a difference, 2 no verdict (a revision that does not
resolve or build, a missing spec, a run that times out, exits 2/101, dumps no
IR or leaves an empty model.c, or a cache without exactly one generated
model.c). A spec that fails to compile or dumps nothing is a failure, never
"identical", and an unusable run exits 2 even when another spec differs.
"""
import argparse, glob, hashlib, json, os, re, shutil, subprocess, sys, tempfile
from dataclasses import dataclass

import contract, tempdirs
from tlxlib import (DEFAULT_JOBS, ROOT, git, include_args, json_update, mode_args, profile_dir,
                    run_proc, tlx_env)

SPECS_DIR = os.path.join(ROOT, "tests", "specs")
PROFILE = "release"
DEFAULT_TIMEOUT = 300
CLIP = 400

# The knobs of one receipt run. TLX_CACHE is a fresh directory per spec and is
# set by run_spec; TLX_JIT=0 keeps the run on the generated C (no Cranelift
# code could hide a codegen difference), TLX_NATIVE_SYNC=1 makes the C build
# synchronous (the generated source is in the cache when the run ends), and
# TLX_IR_VERIFY_HARD=1 turns a rejected IR into a failure instead of a silent
# fallback to the interpreter.
#
# Force compilation (2026-10-08): a small spec whose faithful pre-run
# finishes inside its budget never reaches code generation, so its result is
# the pre-run's and the receipt would be silently EMPTY. TLX_PRERUN_REUSE=0
# disables that shortcut outright; TLX_NATIVE_SYNC=1 and TLX_IR_DUMP=* would
# also force (or trigger) the build, but the receipt must not depend on which
# shortcut a given revision takes, so it turns the reuse off explicitly. A run
# that still dumps no IR, or leaves no single non-empty model.c, is an
# infrastructure failure (exit 2), never an identical receipt: with the JIT
# active the generated source is not on disk at all, so a revision that stops
# writing it (or writes 0 bytes) would otherwise compare two empty files and
# report a receipt that proves nothing. TLX_CGEN_DEBUG stays off on
# purpose: its "why a function got no native code" lines land after the IR dump
# on stderr and would fold unrelated diagnostics into the compared digest.
KNOBS = {
    "TLX_CACHE": "<fresh per spec>",
    "TLX_CACHE_MB": "0",
    "TLX_JIT": "0",
    "TLX_NATIVE_SYNC": "1",
    "TLX_PRERUN_REUSE": "0",
    "TLX_IR_DUMP": "*",
    "TLX_IR_DUMP_RAW": "1",
    "TLX_IR_VERIFY_HARD": "1",
}

DEFAULT_SPECS = [
    ("B6", "invariant violation (exit 12): one INVARIANT and one action property"),
    ("L2", "a `<>` liveness property: the stutter-free search path"),
    ("C3", "a state-level PROPERTY (checked on states, not on actions)"),
    ("L4", "nested `[]<>` and a leads-to `~>`"),
    ("L5", "WF fairness plus `<>done`"),
    ("LC6", "SF fairness and the `[]<>` / `<>[]` implication"),
    ("EK6", "an action property that cannot be evaluated (exit 75)"),
    ("CG4", "encoded-set coercion with the interpreted fallback: INVARIANT + two action properties"),
    ("CMP73A", "0-indexed arrays and three action properties; the largest IR dump of the list"),
]
SPEC_WHY = dict(DEFAULT_SPECS)

# Volatile fields of the compared text: every difference here is not a codegen
# difference. The set follows an earlier worktree-local receipt; its core/memory
# pattern was a no-op replacement there, made a real fold here so a stored receipt is
# also machine-independent (the host core count and fingerprint-memory size are
# properties of the box, not of the generated code). `Finished in \S+ at` also
# covers the 01s/1m03s form of the summary line.
VOLATILE = [
    (re.compile(r"\[pid: \d+\]"), "[pid: N]"),
    (re.compile(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}"), "<ts>"),
    (re.compile(r"\b\d+\.\d+s\b"), "<secs>"),
    (re.compile(r"Finished in \S+ at"), "Finished in <t> at"),
    (re.compile(r"\bon (\d+) cores with (\d+)MB\b"), "on N cores with NMB"),
]

# Exit codes that mean the run itself did not complete: 2 is the CLI usage
# error, 101 an internal error (e.g. a failed symmetry proof). Every other
# code is a verdict (0 success, 10-14 assumption/deadlock/invariant/liveness/
# assert, 75-77 evaluation errors, 150/151 setup/config, 255 general) and is
# compared between the revisions like any other output.
NOT_A_VERDICT = (2, 101)


@dataclass
class Run:
    """One spec run on one side: the artifacts the receipt compares, or the
    `failure` that keeps the run out of a receipt."""
    spec: str
    exit: int | None
    ir: str
    model_c: str
    stdout: str
    failure: str | None = None


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


def file_digest(path):
    with open(path, "rb") as source:
        return hashlib.sha256(source.read()).hexdigest()


def normalize(text, replacements=()):
    for pattern, replacement in VOLATILE:
        text = pattern.sub(replacement, text)
    for literal, token in replacements:
        text = text.replace(literal, token)
    return text


def ir_dump(stderr):
    """The typed IR dump of a run: from its first `ir: ` line to the end of
    stderr, or None when the run printed none (a compile failure)."""
    lines = stderr.splitlines(keepends=True)
    for index, line in enumerate(lines):
        if line.startswith("ir: "):
            return "".join(lines[index:])
    return None


def run_spec(binary, spec, cache, cwd, timeout, replacements):
    """Run one spec through the compiled tier with a fresh cache and collect
    its IR dump, generated model.c, normalized stdout and exit code."""
    env = tlx_env(strip_tlx=True, extra={**KNOBS, "TLX_CACHE": cache})
    command = [binary, "check", spec + ".tla", "-config", spec + ".cfg",
               *include_args(), *mode_args(binary, [], mode="compile")]
    try:
        p = run_proc(command, timeout=timeout, cwd=cwd, env=env)
    except subprocess.TimeoutExpired:
        return Run(spec, None, "", "", "", f"did not finish within {timeout:g}s")
    except OSError as error:
        return Run(spec, None, "", "", "", f"the binary could not run: {error}")
    dump = ir_dump(p.stderr)
    models = sorted(glob.glob(os.path.join(cache, "*", "model.c")))
    model_c = ""
    model_bytes = 0
    if len(models) == 1:
        model_bytes = os.path.getsize(models[0])
        with open(models[0], errors="replace") as source:
            model_c = normalize(source.read(), replacements)
    if p.returncode < 0:
        failure = f"was killed by signal {-p.returncode}"
    elif p.returncode in NOT_A_VERDICT:
        failure = f"did not complete (exit {p.returncode})"
    elif dump is None:
        failure = "dumped no IR (a compile failure, never an identical receipt)"
    elif len(models) != 1:
        failure = f"left {len(models)} generated model.c files in the cache (expected exactly 1)"
    elif not model_bytes:
        failure = "left an empty model.c (0 bytes), never an identical receipt"
    else:
        failure = None
    return Run(spec, p.returncode, normalize(dump or "", replacements), model_c,
               normalize(p.stdout, replacements), failure)


def first_difference(base, candidate):
    """The first differing line of two texts: (line number, base line,
    candidate line), with None for the side that has no such line, or None
    when the texts are equal."""
    left, right = base.splitlines(), candidate.splitlines()
    for index, (a, b) in enumerate(zip(left, right)):
        if a != b:
            return index + 1, a, b
    if len(left) != len(right):
        index = min(len(left), len(right))
        return index + 1, left[index] if index < len(left) else None, right[index] if index < len(right) else None
    return None


def clip(text):
    if text is None:
        return None
    return text if len(text) <= CLIP else text[:CLIP] + "...(truncated)"


def compare_spec(name, base, candidate):
    """The differing fields of one spec: exit code, IR dump, model.c, stdout."""
    differences = []
    if base.exit != candidate.exit:
        differences.append({"spec": name, "field": "exit", "base": base.exit, "candidate": candidate.exit})
    for field, left, right in (("ir", base.ir, candidate.ir), ("model_c", base.model_c, candidate.model_c),
                               ("stdout", base.stdout, candidate.stdout)):
        found = first_difference(left, right)
        if found:
            line, a, b = found
            differences.append({"spec": name, "field": field, "line": line,
                                "base": clip(a), "candidate": clip(b)})
    return differences


def prepare_binary(rev, *, jobs=DEFAULT_JOBS, profile=PROFILE):
    """The release binary of REV: reuse target/ir-receipt/bin/<sha>/<profile>/tlx
    or build it in a temporary detached worktree (verify.py --baseline's
    pattern). Returns (sha, path); raises on a revision that does not resolve
    or build."""
    sha = git("rev-parse", "--verify", rev + "^{commit}")
    cache = os.path.join(ROOT, "target", "ir-receipt", "bin", sha, profile)
    binary = os.path.join(cache, "tlx")
    if os.path.isfile(binary):
        return sha, binary
    os.makedirs(cache, exist_ok=True)
    print(f"ir_receipt: building {rev} ({sha[:12]}, {profile})", file=sys.stderr)
    with tempdirs.TemporaryDirectory(prefix="tlx-ir-receipt-build-") as directory:
        worktree = os.path.join(directory, "tlx")
        git("worktree", "add", "--detach", worktree, sha)
        try:
            env = tlx_env(strip_tlx=True)
            env = {k: v for k, v in env.items() if not k.startswith("GIT_")}
            env["CARGO_TARGET_DIR"] = os.path.join(directory, "target")
            subprocess.run(["cargo", "build", "--profile", profile, "-j", str(jobs)],
                           cwd=worktree, env=env, check=True)
            source = os.path.join(env["CARGO_TARGET_DIR"], profile_dir(profile), "tlx")
            with tempfile.NamedTemporaryFile(dir=cache, delete=False) as output:
                pending = output.name
            try:
                shutil.copy2(source, pending)
                os.replace(pending, binary)
            finally:
                if os.path.exists(pending):
                    os.unlink(pending)
        finally:
            git("worktree", "remove", "--force", worktree)
    return sha, binary


def scratch_specs(specs, directory):
    """A private copy of the selected specs (specs that write next to
    themselves cannot race with another receipt or a test run)."""
    scratch = os.path.join(directory, "specs")
    os.makedirs(scratch)
    for name in specs:
        for suffix in (".tla", ".cfg"):
            shutil.copy2(os.path.join(SPECS_DIR, name + suffix), os.path.join(scratch, name + suffix))
    return scratch


def report_spec(name, base, candidate):
    """The one result line of a spec, plus the first differing lines."""
    if base.failure or candidate.failure:
        why = " / ".join(f"{label}: {run.failure}" for label, run in
                         (("base", base), ("candidate", candidate)) if run.failure)
        print(f"{name}: NO VERDICT ({why})")
        return
    differences = compare_spec(name, base, candidate)
    if not differences:
        print(f"{name}: identical (exit {base.exit}, IR {len(base.ir.splitlines())} lines, "
              f"model.c {len(base.model_c)} B, stdout {len(base.stdout.splitlines())} lines)")
        return
    print(f"{name}: DIFFERS ({', '.join(f'{d['field']} line {d['line']}' for d in differences if 'line' in d) or 'exit code'})")
    for d in differences[:3]:
        if d["field"] == "exit":
            print(f"  exit: base {d['base']} candidate {d['candidate']}")
        else:
            print(f"  {d['field']} line {d['line']}:")
            print(f"    - {d['base']}")
            print(f"    + {d['candidate']}")


def serialize(run):
    return {"exit": run.exit, "failure": run.failure,
            "ir": {"lines": len(run.ir.splitlines()), "bytes": len(run.ir), "sha256": digest(run.ir)},
            "model_c": {"lines": len(run.model_c.splitlines()), "bytes": len(run.model_c),
                        "sha256": digest(run.model_c)},
            "stdout": {"lines": len(run.stdout.splitlines()), "bytes": len(run.stdout),
                       "sha256": digest(run.stdout)}}


def receipt(base_rev, candidate_rev, specs, timeout, out, *, prepare=None, jobs=DEFAULT_JOBS):
    """Build both revisions, run the receipt specs and compare them; the JSON
    summary goes to `out`. Returns the process exit code."""
    prepare = prepare or prepare_binary
    missing = [name for name in specs
               if not os.path.isfile(os.path.join(SPECS_DIR, name + ".tla"))
               or not os.path.isfile(os.path.join(SPECS_DIR, name + ".cfg"))]
    if missing:
        print(f"ir_receipt: no spec or config file for: {', '.join(missing)}", file=sys.stderr)
        return 2
    if timeout <= 0:
        print("ir_receipt: --timeout must be positive", file=sys.stderr)
        return 2
    try:
        base_sha, base_binary = prepare(base_rev, jobs=jobs)
        cand_sha, cand_binary = prepare(candidate_rev, jobs=jobs)
    except (subprocess.CalledProcessError, OSError, ValueError) as error:
        print(f"ir_receipt: cannot build the receipt binaries: {error}", file=sys.stderr)
        return 2
    print(f"ir_receipt: base {base_rev} ({base_sha[:12]}, sha256 {file_digest(base_binary)[:16]}), "
          f"candidate {candidate_rev} ({cand_sha[:12]}, sha256 {file_digest(cand_binary)[:16]})")

    observations = contract.Observations(selected=len(specs), eligible=len(specs))
    differences, results = [], []
    with tempdirs.TemporaryDirectory(prefix="tlx-ir-receipt-") as directory:
        scratch = scratch_specs(specs, directory)
        replacements = [(scratch, "<scratch>"), (os.path.realpath(scratch), "<scratch>"),
                        (ROOT, "<root>"), (os.path.realpath(ROOT), "<root>")]
        runs = {}
        for label, binary in (("base", base_binary), ("candidate", cand_binary)):
            runs[label] = {name: run_spec(binary, name, os.path.join(directory, f"cache-{label}-{name}"),
                                          scratch, timeout, replacements) for name in specs}
    for name in specs:
        base, candidate = runs["base"][name], runs["candidate"][name]
        if base.failure or candidate.failure:
            observations.count(contract.INFRA_FAILED)
        else:
            observations.count()
        report_spec(name, base, candidate)
        if not base.failure and not candidate.failure:
            differences += compare_spec(name, base, candidate)
        results.append({"spec": name, "why": SPEC_WHY.get(name, ""), "identical": not base.failure
                        and not candidate.failure and not compare_spec(name, base, candidate),
                        "base": serialize(base), "candidate": serialize(candidate)})
    refusal = observations.refusal()
    payload = {
        "tool": "ir_receipt",
        "base": {"revision": base_sha, "requested": base_rev, "binary_sha256": file_digest(base_binary)},
        "candidate": {"revision": cand_sha, "requested": candidate_rev, "binary_sha256": file_digest(cand_binary)},
        "specs": [name for name in specs],
        "knobs": KNOBS,
        "results": results,
        "different": sorted({d["spec"] for d in differences}),
        "differences": differences,
        "observations": observations.counts(),
        "identical": not differences and refusal is None,
        "refusal": refusal,
    }
    json_update(out, lambda _: payload)
    identical = sum(1 for r in results if r["identical"])
    print(f"{len(specs)} specs compared, {identical} identical, {len(payload['different'])} different")
    print(f"observations: {observations.summary()}")
    print(f"ir_receipt: receipt written to {out}")
    if refusal:
        print(f"ir_receipt: {refusal}")
    return 2 if refusal else 1 if differences else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("base_rev")
    ap.add_argument("candidate_rev")
    ap.add_argument("--specs", nargs="+", metavar="NAME", default=[name for name, _ in DEFAULT_SPECS],
                    help="specs to compare (default: " + " ".join(name for name, _ in DEFAULT_SPECS) + ")")
    ap.add_argument("--out", required=True, metavar="FILE.json", help="write the receipt JSON here")
    ap.add_argument("--timeout", type=float, default=DEFAULT_TIMEOUT,
                    help=f"wall-time limit per model run (default: {DEFAULT_TIMEOUT}s)")
    a = ap.parse_args()
    sys.exit(receipt(a.base_rev, a.candidate_rev, a.specs, a.timeout, a.out))


if __name__ == "__main__":
    main()
