#!/usr/bin/env python3
"""One-command verification of a revision in a clean checkout.

usage:
  scripts/verify.py [REV] [--quick | --full] [--here] [--keep] [-j N]
                    [--suites NAME,...] [--filter SUBSTR] [--strict] [--cold-cache]
                    [--env K=V]... [--profile P] [--baseline [REV]] [--fuzz-minutes M] [-v]

Checks out REV (default HEAD) into a temporary detached git worktree, or with
--here the current working tree (tracked changes via `git stash create`, which
leaves the stash stack alone, plus untracked files that are not ignored). In
the checkout, `third_party` is a symlink to this tree's third_party and
`corpus/out` a copy of this tree's corpus/out (the TLC oracle), so runs never
overwrite the results here. Then runs the suites of the tier in order:

  suite               quick  full   what
  build                 x     x     cargo build --release (must have no warnings); with a
                                    rustfmt.toml, cargo fmt --all --check (unformatted: a warning);
                                    with [workspace.lints] in Cargo.toml, cargo clippy --workspace
                                    --release --all-targets (each clippy warning is a warning)
  cargo-test            x     x     cargo test --profile test-release (no LTO; no warnings either)
  cargo-test-jit        x     x     TLX_JIT=1 cargo test ... -p tlx-compile --features tlx-testkit/jit
                                    (the JIT as the native tier of the differential tests)
  spectest              x     x     scripts/spectest.py (interpreter)
  spectest-compile      x     x     scripts/spectest.py --extra=-compile (default tiering: JIT, then C)
  spectest-paranoid     x     x     TLX_PARANOID=1 scripts/spectest.py --extra=-compile (C, built first)
  spectest-jit-paranoid x     x     TLX_JIT=1 TLX_PARANOID=1 scripts/spectest.py --extra=-compile (JIT only)
  spectest-c                  x     TLX_JIT=0 TLX_PRERUN_REUSE=0 scripts/spectest.py --extra=-compile
                                    (C only; never the pre-run's interpreter result)
  spectest-compile-w8         x     scripts/spectest.py --extra="-compile -workers 8" (verdicts and
                                    names; counts only for success)
  corpus-compile        q     x     scripts/difftest.py --extra=-compile
  corpus-interp         q     x     scripts/difftest.py (interpreter; --timeout 300 in full)
  corpus-compile-w8           x     scripts/difftest.py --extra=-compile --workers 8 (-j N-1)
  corpus-paranoid                   TLX_PARANOID=1 scripts/difftest.py --extra=-compile (--timeout 600 in full)
  corpus-recover                    scripts/difftest.py --extra=-compile --recover 1 (end each
                                    run after its first checkpoint, recover it)
  corpus-interp-recover             scripts/difftest.py --recover 1 (interpreter)
  corpus-recover-w8                 scripts/difftest.py --extra=-compile --workers 8 --recover 1
  tlcsuite                    x     scripts/tlcsuite.py: TLC's own regression suite against tlx (the
                                    TLC oracle is cached in corpus/out/tlcsuite.json; without it
                                    the suite first runs TLC on every case, about 10 minutes)
  corpus-ext                  x     the small tier of the external corpora (corpus/external.json,
                                    ids ext/...; oracle corpus/out/tlc-ext.json): difftest
                                    --extra=-compile (--timeout 900)
  corpus-ext-interp           x     the same, interpreter (--timeout 1800)
  corpus-ext-w8               x     the same, --extra=-compile --workers 8 (--timeout 900)
  corpus-ext-paranoid               the same, TLX_PARANOID=1 --extra=-compile (--timeout 1800)
  fuzz                        x     scripts/fuzz_spec.py --seed rev: the saved regressions of
                                    tests/fuzz/specs in every mode, 100 generated specs (TLC against
                                    tlx: interpreter, -compile, paranoid, -workers 4, checkpoint and
                                    recover) and 1000 expressions (scripts/fuzz_expr.py); the seed is
                                    derived from the commit, so each revision checks other specs; a
                                    mismatch fails with its minimized repro in the suite's log
                                    directory (needs java; about 3 minutes with -j 3)
  fuzz-campaign                     the same as a campaign: specs until --fuzz-minutes (default
                                    30) have passed, 6000 expressions; for the cloud
                                    (cloud_verify.py --suites build,fuzz-campaign --fuzz-minutes M)
  corpus-ext-large                  the medium and large tiers (oracles tlc-ext.json and
                                    tlc-ext-large.json), --extra=-compile --workers 16 only
                                    (--timeout 3600; for the cloud)
  corpus-ext-medium                 the medium tier, --extra=-compile --workers 16
  corpus-ext-medium-paranoid        the medium tier, TLX_PARANOID=1 --extra=-compile --workers 16

(q: only the curated fast subset in tests/quick_corpus.json; suites
without a mark run only when named with --suites. The corpus-ext* suites
need the fetched corpora, scripts/fetch-external.py; with a --filter that
does not start with ext/ they run nothing.) The default tier is
--quick (a few minutes); --full takes about half an hour, mostly the
interpreter corpus run.

Failures are compared with tests/expected_failures.json (both files are read
from the checked-out revision; from this tree if it has none). An entry
  {"suite": "corpus-*", "id": "SpanningTree/SpanTreeRandom",
   "result": "timeout", "flaky": true, "reason": "..."}
matches failures of models (corpus), specs (spectest) or cases (tlcsuite; its entries are in
tests/tlcsuite_expected_failures.json and have no "suite") in suites matching the
glob `suite`; with `result`, only failures where tlx reported that result
kind (e.g. "timeout") match. An entry without `flaky` must fail: when its
model passes, it is reported as FIXED (remove the entry; always fatal for case drivers).

Prints a summary table (suite, passed/total, unexpected failures, expected
failures, duration) and exits nonzero on unexpected failures, compiler
warnings or a suite that did not run to the end. Logs and summary.json are
kept in target/verify/<time>-<rev>/ of this tree. The worktree is removed
unless --keep. Test runs use at most -j N jobs (default 3).

Other options: --suites runs just the named suites (with the tier's corpus
selection); --filter runs only the corpus models whose id contains SUBSTR
(e.g. `--full --filter DieHard` to try the full tier cheaply); --cold-cache builds generated C into a fresh TLX_CACHE instead of
~/.cache/tlx; --env K=V (repeatable) sets K in the environment of the tlx
runs of the spec/corpus/cargo-test suites, which is the only way to pass a
TLX_* variable through (all others are stripped so that a stray one in the
calling shell cannot change a result); it is recorded in summary.json;
--profile P is the cargo profile of the tlx binary the spec and corpus suites
run (default release: the build suite has built it already, and it runs
faster than the fast test-release profile that spectest/difftest use by
default; docs/testing.md §3); -v streams the suites' output.
"""
import tempdirs

import argparse, ast, json, os, re, shutil, subprocess, sys, tempfile, time
from pathlib import Path

from tlc import compile_fallback
from tlxlib import ROOT, DEFAULT_JOBS, git, fmt_secs, tlx_env, run_proc, expected_match, TEST_LINE, read_case_report, profile_dir


def module_imports(path, package=""):
    """Names a harness module imports that could be a sibling scripts/ module.

    A level-0 `import X` / `from X import ...` names a top-level module; a
    relative import inside a package names its own siblings. Nested imports
    count too (oracle imports corpus inside a function), because the overlay
    must bring a dependency the older checkout may not have at all."""
    names = set()
    for node in ast.walk(ast.parse(path.read_text(errors="replace"))):
        if isinstance(node, ast.Import):
            names.update(alias.name for alias in node.names)
        elif isinstance(node, ast.ImportFrom) and node.level == 0 and node.module:
            names.add(node.module)
            names.update(f"{node.module}.{alias.name}" for alias in node.names)
        elif isinstance(node, ast.ImportFrom) and node.level:
            parts = package.split(".") if package else []
            base = parts[:len(parts) - (node.level - 1)]
            if node.module:
                base = base + [node.module]
            if base:
                names.add(".".join(base))
            names.update(".".join(base + [alias.name]) if base else alias.name for alias in node.names)
    return names


def module_package(scripts_dir, path):
    """The dotted package of module file `path` under `scripts_dir` ("" at top level)."""
    return ".".join(path.relative_to(scripts_dir).parts[:-1])


def local_module(scripts_dir, name):
    """The file of sibling module `name` under `scripts_dir`, or None."""
    base = Path(scripts_dir).joinpath(*name.split("."))
    for candidate in (base.with_suffix(".py"), base / "__init__.py"):
        if candidate.is_file():
            return candidate
    return None


def overlay_files(scripts_dir, seeds):
    """Repo-relative paths of `seeds` plus every sibling module they import.

    The old-revision overlay (see its note in main) copies this tree's drivers
    into an older checkout; their local imports must travel with them, or the
    overlaid driver imports an older or missing module. Resolving the closure
    keeps the source-relative layout, so a module that later moves into
    scripts/<package>/ still lands where its import expects it (HARNESS-13)."""
    scripts_dir = Path(scripts_dir)
    pending, seen, paths = list(seeds), set(), set()
    while pending:
        name = pending.pop()
        if name in seen:
            continue
        seen.add(name)
        path = local_module(scripts_dir, name)
        if path is None:
            continue
        paths.add(path)
        pending.extend(sorted(module_imports(path, module_package(scripts_dir, path))))
    # A module inside a package needs the package marker to be importable.
    for path in list(paths):
        for parent in path.parents:
            if parent == scripts_dir:
                break
            marker = parent / "__init__.py"
            if marker.is_file():
                paths.add(marker)
    return sorted(os.path.relpath(path, scripts_dir.parent) for path in paths)


QUICK_CORPUS = os.path.join("tests", "quick_corpus.json")
EXPECTED = os.path.join("tests", "expected_failures.json")
TLCSUITE_EXPECTED = os.path.join("tests", "tlcsuite_expected_failures.json")
# The --filter of the external-corpus suites (the ids of corpus/external.json's models).
EXT = "ext/"

# name: (tiers, kind, extra arguments, environment)
SUITES = [
    ("build", "qf", "build", [], {}),
    ("cargo-test", "qf", "cargo-test", [], {}),
    ("cargo-test-jit", "qf", "cargo-test", ["-p", "tlx-compile", "--features", "tlx-testkit/jit"], {"TLX_JIT": "1"}),
    ("spectest", "qf", "spec", [], {}),
    ("spectest-compile", "qf", "spec", ["--extra=-compile"], {}),
    ("spectest-paranoid", "qf", "spec", ["--extra=-compile"], {"TLX_PARANOID": "1"}),
    ("spectest-jit-paranoid", "qf", "spec", ["--extra=-compile"], {"TLX_JIT": "1", "TLX_PARANOID": "1"}),
    ("spectest-c", "f", "spec", ["--extra=-compile"], {"TLX_JIT": "0", "TLX_PRERUN_REUSE": "0"}),
    ("spectest-compile-w8", "f", "spec", ["--extra=-compile -workers 8"], {}),
    ("corpus-compile", "qf", "corpus", ["--extra=-compile", "--tag=-compile"], {}),
    ("corpus-interp", "qf", "corpus", [], {}),
    # The default mode (no -tlc) compared with TLC by verdict (docs/reductions.md §1.2).
    ("spectest-default", "qf", "spec", ["--default-mode"], {}),
    ("corpus-default", "qf", "corpus", ["--default-mode", "--tag=-default"], {}),
    ("corpus-compile-w8", "f", "corpus", ["--extra=-compile", "--tag=-compile-w8", "--workers", "8"], {}),
    ("tlcsuite", "f", "tlcsuite", [], {}),
    # The external corpora (docs/testing.md 6.3): difftest selects them by --filter ext/.
    # The small tier in every mode; the medium and large tiers only with 16 workers (user decision 2026-10-06).
    ("corpus-ext", "f", "corpus", ["--extra=-compile", "--tag=-ext-compile", "--filter", EXT, "--tiers", "small", "--timeout", "900"], {}),
    ("corpus-ext-interp", "f", "corpus", ["--tag=-ext-interp", "--filter", EXT, "--tiers", "small", "--timeout", "1800"], {}),
    ("corpus-ext-w8", "f", "corpus", ["--extra=-compile", "--tag=-ext-compile-w8", "--workers", "8", "--filter", EXT, "--tiers", "small",
                                      "--timeout", "900"], {}),
    ("corpus-ext-paranoid", "", "corpus", ["--extra=-compile", "--tag=-ext-paranoid", "--filter", EXT, "--tiers", "small", "--timeout", "1800"],
     {"TLX_PARANOID": "1"}),
    ("corpus-ext-large", "", "corpus", ["--extra=-compile", "--tag=-ext-large-w16", "--workers", "16", "--filter", EXT,
                                        "--tiers", "medium,large", "--oracle", "tlc-ext-large", "--timeout", "3600"], {}),
    ("corpus-ext-medium", "", "corpus", ["--extra=-compile", "--tag=-ext-medium", "--workers", "16", "--filter", EXT,
                                         "--tiers", "medium", "--timeout", "3600"], {}),
    ("corpus-ext-medium-paranoid", "", "corpus", ["--extra=-compile", "--tag=-ext-medium-paranoid", "--workers", "16", "--filter", EXT,
                                                  "--tiers", "medium", "--timeout", "3600"], {"TLX_PARANOID": "1"}),
    ("corpus-paranoid", "", "corpus", ["--extra=-compile", "--tag=-paranoid"], {"TLX_PARANOID": "1"}),
    # Differential fuzzing against TLC (docs/testing.md section 7): bounded in the full tier, a timed campaign by name.
    ("fuzz", "f", "fuzz", ["--seed", "rev", "--count", "100", "--expr", "1000", "--replay", "--shrink", "2"], {}),
    ("fuzz-campaign", "", "fuzz", ["--seed", "rev", "--count", "1000000", "--expr", "6000", "--replay", "--shrink", "12",
                                    "--shrink-minutes", "4"], {}),
    # Checkpoint/recover: every run ended after its first checkpoint
    # and recovered (only when named with --suites).
    ("corpus-recover", "", "corpus", ["--extra=-compile", "--tag=-recover", "--recover", "1"], {}),
    ("corpus-interp-recover", "", "corpus", ["--tag=-interp-recover", "--recover", "1"], {}),
    ("corpus-recover-w8", "", "corpus", ["--extra=-compile", "--tag=-recover-w8", "--workers", "8", "--recover", "1"], {}),
]

class Checkout:
    """A temporary detached worktree of `rev` (with `here`: of the working tree)."""

    def __init__(self, rev, here, keep):
        self.keep = keep
        self.tmp = tempdirs.mkdtemp(prefix="tlx-verify-")
        self.path = os.path.join(self.tmp, "tlx")
        if here:
            # A commit of the working tree's tracked changes (not stored on the stash stack).
            self.sha = git("stash", "create") or git("rev-parse", "HEAD")
            self.label = "here"
        else:
            self.sha = git("rev-parse", "--verify", rev + "^{commit}")
            self.label = git("rev-parse", "--short", self.sha)
        git("worktree", "add", "--detach", self.path, self.sha)
        try:
            self.populate(here)
        except BaseException:
            self.keep = False
            self.remove()
            raise

    def populate(self, here):
        if here:
            out = subprocess.run(["git", "ls-files", "--others", "--exclude-standard", "-z"], cwd=ROOT,
                                 check=True, capture_output=True).stdout.decode()
            for f in filter(None, out.split("\0")):
                dst = os.path.join(self.path, f)
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                shutil.copy2(os.path.join(ROOT, f), dst)
        tp = os.path.join(ROOT, "third_party")
        if os.path.exists(tp):
            os.symlink(os.path.realpath(tp), os.path.join(self.path, "third_party"))
        out = os.path.join(ROOT, "corpus", "out")
        if os.path.isdir(out):
            shutil.copytree(os.path.realpath(out), os.path.join(self.path, "corpus", "out"))

    def remove(self):
        if self.keep:
            print(f"worktree kept: {self.path}  (remove with: git worktree remove --force {self.path})")
            return
        subprocess.run(["git", "worktree", "remove", "--force", self.path], cwd=ROOT, capture_output=True)
        shutil.rmtree(self.tmp, ignore_errors=True)


class Result:
    def __init__(self, name):
        self.name = name
        self.passed = []      # ids
        self.failed = {}      # id -> (tlx result kind or None, output line)
        self.warnings = []    # compiler warning lines
        self.error = None     # the suite did not run to the end
        self.secs = 0.0
        self.unexpected, self.expected, self.fixed = [], [], []
        self.preexisting = []


def run_logged(cmd, cwd, env, log, verbose):
    """Run `cmd`, writing its output to `log` (and stdout with `verbose`): (exit code, output)."""
    with open(log, "w") as f:
        f.write("$ " + " ".join(cmd) + "\n")
        f.flush()
        def write(line):
            f.write(line)
            f.flush()
            if verbose or re.match(r"\s*(?:FAIL|FAILED|XPASS|ERROR|TIMEOUT)\b", line):
                sys.stdout.write("    " + line)
                sys.stdout.flush()
        result = run_proc(cmd, cwd=cwd, env={**env, "PYTHONUNBUFFERED": "1"}, on_line=write)
        return result.returncode, result.stdout


def warning_lines(out):
    return [l for l in out.splitlines() if re.match(r"warning(\[\w+\])?: ", l) and "generated" not in l]


def has_workspace_lints(wt):
    """Whether the checkout's root Cargo.toml declares [workspace.lints] (older revisions don't: no clippy gate)."""
    try:
        with open(os.path.join(wt, "Cargo.toml")) as f:
            return any(l.strip().startswith("[workspace.lints") for l in f)
    except OSError:
        return False


def profile_args(ctx, script):
    """The arguments that make the checkout's `script` build and run the tlx
    binary of ctx.profile (before --profile existed, spectest always used the
    release binary and difftest did with --release)."""
    with open(os.path.join(ctx.wt, "scripts", script)) as source:
        if "add_profile_args" in source.read():
            return ["--profile", ctx.profile]
    return ["--release"] if script == "difftest.py" else []


def run_suite(name, kind, args, extra_env, ctx):
    r = Result(name)
    env = tlx_env(strip_tlx=True)
    env.pop("CARGO_TARGET_DIR", None)
    # --env reaches the tlx runs of these suites (the suite's own environment
    # wins for a variable it defines, e.g. spectest-paranoid's TLX_PARANOID).
    if kind in ("spec", "corpus", "cargo-test", "tlcsuite", "fuzz"):
        env.update(ctx.env)
    env.update(extra_env)
    if ctx.cache:
        env["TLX_CACHE"] = ctx.cache
    log = os.path.join(ctx.logdir, name + ".log")
    wt, j = ctx.wt, str(ctx.jobs)
    t0 = time.time()
    report_path = log + ".cases.json"
    result_args = ["--results-json", report_path, "--suite", name]
    if os.path.exists(report_path):
        os.unlink(report_path)
    if kind == "build":
        rc, out = run_logged(["cargo", "build", "--release", "-j", j], wt, env, log, ctx.verbose)
        r.warnings = warning_lines(out)
        if rc == 0:
            r.passed.append("cargo build")
        else:
            r.error = f"cargo build failed (exit {rc})"
        # Revisions with a rustfmt.toml are kept formatted (B1-fmt, 2026-10-06).
        if os.path.exists(os.path.join(wt, "rustfmt.toml")):
            frc, fout = run_logged(["cargo", "fmt", "--all", "--check"], wt, env, log + ".fmt", ctx.verbose)
            if frc != 0:
                files = sorted({l.split(":")[0][len("Diff in "):] for l in fout.splitlines() if l.startswith("Diff in ")})
                r.warnings.append(f"cargo fmt --check: {len(files)} file(s) not formatted" + (f" ({', '.join(files[:5])})" if files else ""))
        # Revisions that declare their lint set ([workspace.lints], B1, 2026-10-06) are clippy-clean.
        if has_workspace_lints(wt):
            crc, cout = run_logged(["cargo", "clippy", "--workspace", "--release", "--all-targets", "-j", j, "--", "-D", "warnings"], wt, env, log + ".clippy", ctx.verbose)
            r.warnings += warning_lines(cout)
            if crc != 0 and not r.error:
                r.error = f"cargo clippy failed (exit {crc})"
    elif kind == "cargo-test":
        if name == "cargo-test":
            rc, _ = run_logged([sys.executable, "-m", "unittest", "discover", "-s", "scripts", "-p", "test_*.py"],
                               wt, env, log + ".python", ctx.verbose)
            if rc != 0:
                r.error = f"Python driver tests failed (exit {rc})"
                r.secs = time.time() - t0
                return r
        # The test-release profile (if the checkout has it) skips LTO: much faster to build.
        with open(os.path.join(wt, "Cargo.toml")) as manifest:
            profile = ["--profile", "test-release"] if "[profile.test-release]" in manifest.read() else ["--release"]
        rc, out = run_logged(["cargo", "test", *profile, "-j", j, *args, "--", "--test-threads", j], wt, env, log, ctx.verbose)
        r.warnings = warning_lines(out)
        binary = "?"
        for line in out.splitlines():
            if m := re.match(r"\s*(?:Running \S+ \((\S+)\)|Doc-tests (\S+))", line):
                binary = os.path.basename(m.group(1) or m.group(2)).rsplit("-", 1)[0]
            elif m := TEST_LINE.match(line):
                tid = f"{binary}::{m.group(1)}"
                if m.group(2) == "ok":
                    r.passed.append(tid)
                elif m.group(2) == "FAILED":
                    r.failed[tid] = (None, line)
        if rc != 0 and not r.failed:
            r.error = f"cargo test failed (exit {rc})"
    elif kind == "spec":
        rc, out = run_logged([sys.executable, "scripts/spectest.py", *profile_args(ctx, "spectest.py"), *result_args, *args], wt, env, log, ctx.verbose)
        consume_case_report(r, report_path, rc)
    elif kind == "tlcsuite":
        cmd = [sys.executable, "scripts/tlcsuite.py", *profile_args(ctx, "tlcsuite.py"), "-v", "-j", j, *result_args, *args]
        rc, out = run_logged(cmd, wt, env, log, ctx.verbose)
        consume_case_report(r, report_path, rc)
    elif kind == "fuzz":
        if not os.path.exists(os.path.join(wt, "scripts", "fuzz_spec.py")):
            print(f"note: the checkout has no scripts/fuzz_spec.py, suite {name} skipped")
            r.secs = time.time() - t0
            return r
        cmd = [sys.executable, "scripts/fuzz_spec.py", *profile_args(ctx, "fuzz_spec.py"), "-j", j,
               "--out", os.path.join(ctx.logdir, name), *result_args, *args]
        if name == "fuzz-campaign":
            cmd += ["--minutes", str(ctx.fuzz_minutes)]
        rc, out = run_logged(cmd, wt, env, log, ctx.verbose)
        consume_case_report(r, report_path, rc)
    elif kind == "corpus":
        ext = EXT in args
        if ext:
            # (Their own --filter; a --filter of the call narrows it only if it names ext/ models.)
            if ctx.filter and not ctx.filter.startswith(EXT):
                r.error = "zero selected cases (external suite excluded by filter)"
                r.secs = time.time() - t0
                return r
            if ctx.filter:
                args = [ctx.filter if a == EXT else a for a in args]
        if "--workers" in args and ext:
            # (Long runs of the external corpora: about one core per worker.)
            j = str(max(1, ctx.jobs // int(args[args.index("--workers") + 1])))
        elif "--workers" in args:
            # Each run uses several threads: fewer runs at a time.
            j = str(max(1, ctx.jobs - 1))
        # (-v: the log keeps the end of the output of each failing model.)
        cmd = [sys.executable, "scripts/difftest.py", *profile_args(ctx, "difftest.py"), "-v", "-j", j, *result_args, *args]
        if name in ("corpus-interp", "corpus-interp-recover"):
            cmd += ["--timeout", "300" if ctx.tier == "full" else "120"]
        elif name == "corpus-paranoid" and ctx.tier == "full":
            # (Each state is also run by the interpreter.)
            cmd += ["--timeout", "600"]
        if ctx.tier == "quick" and not ext:
            cmd += ["--ids", ",".join(ctx.quick_corpus)]
        if ctx.filter and not ext:
            cmd += ["--filter", ctx.filter]
        rc, out = run_logged(cmd, wt, env, log, ctx.verbose)
        consume_case_report(r, report_path, rc)
    if fallback := compile_fallback(out):
        r.failed["whole-model compiler fallback"] = ("compile failure", fallback)
        r.error = fallback
        print(f"FAIL {name}: {fallback}", flush=True)
    r.secs = time.time() - t0
    return r


def consume_case_report(r, path, rc):
    try:
        cases, report = read_case_report(path, rc)
    except (OSError, ValueError, KeyError, TypeError) as error:
        r.error = f"missing or invalid case report: {error} (exit {rc})"
        return
    r.case_report = report
    for case in cases:
        message = case.details.get("message", case.status)
        if "oracle" in str(message).lower() and any(word in str(message).lower() for word in ("missing", "stale", "incomplete")):
            message = str(message) + "; refresh shared oracles with: sh harness/agent_setup.sh --refresh-oracles"
        if case.status == "pass":
            r.passed.append(case.id)
        elif case.status == "xfail":
            r.expected.append((case.id, case.details.get("expected", message)))
        elif case.status in ("fail", "xpass", "error", "timeout"):
            r.unexpected.append((case.id, message))
            if case.status == "xpass":
                r.fixed.append((case.id, case.details.get("expected", message)))
    if report["exit_code"] or report["errors"]:
        r.error = "; ".join(report["errors"]) or "case judgment failed"


def classify(r, expected):
    """Split the failures of `r` into unexpected and expected ones; find fixed entries."""
    if hasattr(r, "case_report"):
        return  # The driver already used the shared expected-failure policy.
    for tid, (kind, line) in sorted(r.failed.items()):
        e = expected_match(expected, r.name, tid, kind)
        if e:
            r.expected.append((tid, e["reason"]))
        else:
            r.unexpected.append((tid, line))
    for tid in r.passed:
        e = expected_match(expected, r.name, tid, match_result=False)
        if e and not e.get("flaky"):
            r.fixed.append((tid, e["reason"]))


def baseline_binary(rev, profile, jobs, *, root=ROOT):
    """Build REV in a temporary detached worktree; cache its binary by commit/profile."""
    root = os.path.abspath(root)
    def base_git(*args):
        return subprocess.check_output(["git", *args], cwd=root, text=True).strip()
    sha = base_git("rev-parse", "--verify", rev + "^{commit}")
    # Profiles are Cargo names, not arbitrary paths into the cache.
    if not re.fullmatch(r"[A-Za-z0-9_-]+", profile):
        raise ValueError(f"invalid Cargo profile: {profile!r}")
    cache = os.path.join(root, "target", "verify-baseline", sha, profile)
    binary = os.path.join(cache, "tlx")
    if os.path.isfile(binary):
        return binary
    os.makedirs(cache, exist_ok=True)
    with tempdirs.TemporaryDirectory(prefix="tlx-baseline-") as directory:
        worktree = os.path.join(directory, "tlx")
        base_git("worktree", "add", "--detach", worktree, sha)
        try:
            # A fresh build directory cannot reuse stale extracted-source mtimes.
            env = tlx_env(strip_tlx=True)
            env = {k: v for k, v in env.items() if not k.startswith("GIT_")}
            env["CARGO_TARGET_DIR"] = os.path.join(directory, "target")
            subprocess.run(["cargo", "build", "--profile", profile, "-j", str(jobs)], cwd=worktree, env=env, check=True)
            source = os.path.join(env["CARGO_TARGET_DIR"], profile_dir(profile), "tlx")
            # Atomic publication: interruption never leaves a partial cache entry.
            with tempfile.NamedTemporaryFile(dir=cache, delete=False) as output:
                pending = output.name
            try:
                shutil.copy2(source, pending)
                os.replace(pending, binary)
            finally:
                if os.path.exists(pending):
                    os.unlink(pending)
        finally:
            base_git("worktree", "remove", "--force", worktree)
    return binary


def triage_baseline(r, kind, args, extra_env, ctx, binary, rev):
    """Rerun exactly failing cases with the current driver/specs and REV's binary.

    XPASS and infrastructure/coverage errors remain fatal: passing an old binary
    is not evidence of a pre-existing semantic failure.
    """
    if kind not in ("spec", "corpus") or not r.unexpected:
        return
    fixed = {tid for tid, _ in r.fixed}
    ids = [tid for tid, _ in r.unexpected if tid not in fixed]
    if not ids:
        return
    script = "spectest" if kind == "spec" else "difftest"
    # The drivers already expose run functions, but not a binary CLI option.
    # Override only their build context; all case judgment and timeout code runs
    # unchanged in the checked-out revision's driver.
    bridge = (
        "import contextlib, importlib, sys; "
        "sys.path.insert(0, 'scripts'); "
        "driver = importlib.import_module(sys.argv.pop(1)); "
        "binary = sys.argv.pop(1); "
        "driver.build_tlx = lambda *a, **kw: contextlib.nullcontext(binary); "
        "driver.main()"
    )
    path = os.path.join(ctx.logdir, r.name + ".baseline.cases.json")
    if os.path.exists(path):
        os.unlink(path)
    arguments = [*args, "--results-json", path, "--suite", r.name]
    if kind == "spec":
        arguments += ids
    else:
        # Override the tier's selection and tag so current result files are preserved.
        arguments += ["--ids", ",".join(ids), "--tag=-baseline", "-j", str(ctx.jobs)]
        if r.name in ("corpus-interp", "corpus-interp-recover"):
            arguments += ["--timeout", "300" if ctx.tier == "full" else "120"]
        elif r.name == "corpus-paranoid" and ctx.tier == "full":
            arguments += ["--timeout", "600"]
    env = tlx_env(strip_tlx=True)
    env.update(ctx.env)
    env.update(extra_env)
    if ctx.cache:
        env["TLX_CACHE"] = ctx.cache
    rc, _ = run_logged([sys.executable, "-c", bridge, script, binary, *arguments], ctx.wt, env,
                       os.path.join(ctx.logdir, r.name + ".baseline.log"), ctx.verbose)
    try:
        cases, report = read_case_report(path, rc)
        # Nonzero is expected for failing cases, but not collection/driver errors.
        errors = [e for e in report["errors"] if e != "driver failed (exit 1)"]
        if errors or set(report["selected"]) != set(ids):
            raise ValueError("baseline driver did not execute exactly the failing cases: " + "; ".join(errors))
    except (OSError, ValueError, KeyError, TypeError) as error:
        r.error = f"baseline rerun failed: {error}"
        return
    failed = {case.id for case in cases if case.status in ("fail", "xfail", "error", "timeout")}
    r.preexisting = []
    new = []
    for tid, message in r.unexpected:
        if tid in failed:
            label = f"pre-existing (fails on {rev} too): {message}"
            r.preexisting.append((tid, label))
            print(f"\nPRE-EXISTING {r.name}: {tid}: {label}", flush=True)
        else:
            new.append((tid, "new: " + message))
    r.unexpected = new
    # Only forgive the case driver's ordinary failure exit, never missing cases,
    # coverage errors, or exit/report disagreement.
    if getattr(r, "case_report", {}).get("errors") == ["driver failed (exit 1)"]:
        r.error = None


def result_failed(r):
    return bool(r.unexpected or r.warnings or r.error)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("rev", nargs="?", default="HEAD")
    tier = ap.add_mutually_exclusive_group()
    tier.add_argument("--quick", dest="tier", action="store_const", const="quick")
    tier.add_argument("--full", dest="tier", action="store_const", const="full")
    ap.add_argument("--baseline", nargs="?", const="", default=None, metavar="REV",
                    help="triage failing spec/corpus cases against REV (default merge-base with main)")
    ap.add_argument("--here", action="store_true", help="verify the working tree (with uncommitted changes)")
    ap.add_argument("--keep", action="store_true", help="keep the worktree")
    ap.add_argument("-j", "--jobs", type=int, default=DEFAULT_JOBS)
    ap.add_argument("--filter", default="", help="only corpus models whose id contains this")
    ap.add_argument("--suites", default="", help="comma-separated suites to run (default: the tier's)")
    ap.add_argument("--strict", action="store_true", help="FIXED cargo tests are fatal too (case drivers always reject XPASS)")
    ap.add_argument("--cold-cache", action="store_true", help="fresh TLX_CACHE for generated C")
    ap.add_argument("--env", action="append", default=[], metavar="K=V",
                    help="environment variable for the tlx runs of the spec/corpus/cargo-test suites (repeatable)")
    ap.add_argument("--profile", default="release", help="cargo profile of the tlx binary of the spec/corpus suites")
    ap.add_argument("--fuzz-minutes", type=float, default=30, help="duration of the fuzz-campaign suite's generation")
    ap.add_argument("-v", "--verbose", action="store_true")
    a = ap.parse_args()
    a.tier = a.tier or "quick"
    env = {}
    for pair in a.env:
        k, sep, v = pair.partition("=")
        if not sep or not k:
            ap.error(f"--env expects K=V, got {pair!r}")
        env[k] = v
    names = {s[0] for s in SUITES}
    wanted = [s for s in a.suites.split(",") if s]
    if bad := [s for s in wanted if s not in names]:
        ap.error(f"unknown suite(s) {bad}; known: {', '.join(s[0] for s in SUITES)}")
    suites = [s for s in SUITES if (s[0] in wanted if wanted else a.tier[0] in s[1])]

    co = Checkout(a.rev, a.here, a.keep)

    class Ctx:
        pass

    ctx = Ctx()
    ctx.wt, ctx.jobs, ctx.tier, ctx.verbose, ctx.filter = co.path, a.jobs, a.tier, a.verbose, a.filter
    ctx.env = env
    ctx.profile = a.profile
    ctx.fuzz_minutes = a.fuzz_minutes
    ctx.logdir = os.path.join(ROOT, "target", "verify", time.strftime("%Y%m%d-%H%M%S") + "-" + co.label)
    os.makedirs(ctx.logdir, exist_ok=True)
    ctx.cache = os.path.join(co.tmp, "tlx-cache") if a.cold_cache else None
    results = []
    base_binary = None
    baseline = (a.baseline or git("merge-base", "main", co.sha)) if a.baseline is not None else None
    t_all = time.time()
    try:
        def config(rel):
            path = os.path.join(co.path, rel)
            if not os.path.exists(path):
                print(f"note: {rel} not in the checkout, using this tree's")
                path = os.path.join(ROOT, rel)
            with open(path) as stream:
                return json.load(stream)

        ctx.quick_corpus = config(QUICK_CORPUS)["models"]
        expected = config(EXPECTED)["expected_failures"]
        # (The tlcsuite entries have no "suite" field: it is this suite's.)
        expected += [{"suite": "tlcsuite", **e} for e in config(TLCSUITE_EXPECTED)["expected_failures"]]
        dt = os.path.join(co.path, "scripts", "difftest.py")
        tiers = any("--tiers" in s[3] for s in suites)
        with open(dt) as stream:
            difftest_flags = stream.read()
        if (a.tier == "quick" and "--ids" not in difftest_flags) or (tiers and "--tiers" not in difftest_flags):
            print("note: the checkout's difftest.py has no --ids or --tiers, overlaying this tree's drivers")
            # The whole dependency closure travels with the seeds, so the overlaid
            # driver never imports a module the older checkout does not have.
            for rel in overlay_files(os.path.join(ROOT, "scripts"), ("difftest", "tlxlib", "tlc")):
                destination = os.path.join(co.path, rel)
                os.makedirs(os.path.dirname(destination), exist_ok=True)
                shutil.copy2(os.path.join(ROOT, rel), destination)
        print(f"verify {co.label} ({co.sha[:12]}) tier={a.tier} profile={a.profile}"
              + (f" env={','.join(f'{k}={v}' for k, v in env.items())}" if env else "")
              + f" in {co.path}\nlogs: {ctx.logdir}")
        for name, _, kind, args, suite_env in suites:
            print(f"  {name:20} ...", end="", flush=True)
            r = run_suite(name, kind, args, suite_env, ctx)
            classify(r, expected)
            if baseline is not None and kind in ("spec", "corpus") and r.unexpected:
                try:
                    if base_binary is None:
                        base_binary = baseline_binary(baseline, ctx.profile, ctx.jobs)
                    triage_baseline(r, kind, args, suite_env, ctx, base_binary, baseline)
                except (OSError, ValueError, subprocess.CalledProcessError) as error:
                    r.error = f"cannot build baseline: {error}"
            results.append(r)
            status = "ok" if not result_failed(r) else "FAIL"
            print(f" {status} ({fmt_secs(r.secs)})", flush=True)
            if kind == "build" and r.error:
                break
    finally:
        co.remove()
    total = time.time() - t_all

    print(f"\n{'suite':20} {'passed/total':>13} {'unexpected':>10} {'expected':>8} {'fixed':>5} {'warn':>4} {'time':>7}")
    for r in results:
        n = len(r.case_report["selected"]) if hasattr(r, "case_report") else len(r.passed) + len(r.failed)
        print(f"{r.name:20} {f'{len(r.passed)}/{n}':>13} {len(r.unexpected):>10} {len(r.expected):>8} "
              f"{len(r.fixed):>5} {len(r.warnings):>4} {fmt_secs(r.secs):>7}" + (f"  ERROR: {r.error}" if r.error else ""))
    print(f"{'total':20} {'':>13} {sum(len(r.unexpected) for r in results):>10} {sum(len(r.expected) for r in results):>8} "
          f"{sum(len(r.fixed) for r in results):>5} {sum(len(r.warnings) for r in results):>4} {fmt_secs(total):>7}")
    for r in results:
        for tid, reason in r.preexisting:
            print(f"PRE-EXISTING {r.name}: {tid}\n    {reason}")
    for r in results:
        for w in r.warnings:
            print(f"WARNING   {r.name}: {w}")
        for tid, line in r.unexpected:
            print(f"UNEXPECTED {r.name}: {tid}\n    {line.strip()}")
        for tid, reason in r.fixed:
            print(f"FIXED     {r.name}: {tid} passes; remove its expected_failures entry ({reason})")
        for tid, reason in r.expected:
            print(f"expected  {r.name}: {tid} ({reason})")
    bad = any(result_failed(r) for r in results) or (a.strict and any(r.fixed for r in results))
    summary = {"rev": co.sha, "label": co.label, "tier": a.tier, "env": env, "profile": a.profile, "baseline": baseline, "ok": not bad, "secs": round(total),
               "suites": [{"name": r.name, "passed": len(r.passed), "total": len(r.case_report["selected"]) if hasattr(r, "case_report") else len(r.passed) + len(r.failed),
                           "case_counts": getattr(r, "case_report", {}).get("counts", {}),
                           "coverage": getattr(r, "case_report", {}).get("coverage", {}),
                           "secs": round(r.secs), "error": r.error, "warnings": r.warnings,
                           "preexisting": [t for t, _ in r.preexisting],
                           "unexpected": [t for t, _ in r.unexpected], "expected": [t for t, _ in r.expected],
                           "fixed": [t for t, _ in r.fixed]} for r in results]}
    with open(os.path.join(ctx.logdir, "summary.json"), "w") as out:
        json.dump(summary, out, indent=1)
    print("\nVERIFY " + ("FAILED" if bad else "PASSED") + f" ({co.label}, {a.tier}, {fmt_secs(total)})")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
