"""Small real-model and mutant checks for autosym_mutants --guard-fixtures."""
import json
from pathlib import Path
import shutil

import tempdirs
from tlc import parse, run_tlc
from tlxlib import ROOT, mode_args, run_proc, tlx_env

FIXTURES = Path(ROOT) / "tests" / "autosym"
SOURCES = {
    "PaxosCommit": "Examples/specifications/transaction_commit",
    "VR_STATE_TRANSFER": "external/vanlightly-vsr/analysis/03-state-transfer",
    "VR_APP_STATE": "external/vanlightly-vsr/analysis/04-application-state",
}
FIELDS = ("result", "violated", "generated", "distinct", "depth", "trace_length")
TIERS = (
    ("interpreter", ["-interp"], {}),
    ("closure", ["-compile"], {"TLX_NO_NATIVE": "1"}),
    ("C", ["-compile"], {"TLX_JIT": "0", "TLX_NATIVE_SYNC": "1"}),
    ("JIT", ["-compile"], {"TLX_JIT": "only"}),
    ("paranoid", ["-compile"], {"TLX_PARANOID": "1"}),
)


def signature(result):
    return {key: result.get(key) for key in FIELDS}


def check_fixtures(binary, timeout, refresh, report_path):
    """Compare guarded runs with small TLC declared-symmetry oracles.

    Missing inputs/oracles and compiler fallback are failures, never skips.
    The restart mutant is compared with an actual unreduced run per tier.
    """
    cache_path = FIXTURES / "expected.json"
    expected = json.loads(cache_path.read_text()) if cache_path.exists() else {}
    reports = []
    with tempdirs.TemporaryDirectory(prefix="tlx-autosym-guards-") as tmp:
        work = Path(tmp)
        for name, source in SOURCES.items():
            source = Path(ROOT) / "third_party" / source
            for path in source.glob("*.tla"):
                shutil.copyfile(path, work / path.name)
            cfg = (FIXTURES / (name + ".cfg")).read_text()
            tla = work / (name + ".tla")
            symmetry = "symmValues"
            if name == "PaxosCommit":
                symmetry = "AutosymFixtureGroup"
                text = tla.read_text().replace("EXTENDS Integers", "EXTENDS Integers, TLC")
                end = text.rfind("====")
                # Insert before the final module delimiter, not inside its '=' run.
                end = text.rfind("\n", 0, end) + 1
                text = text[:end] + symmetry + " == Permutations(Acceptor)\n" + text[end:]
                tla.write_text(text)
            plain_cfg = work / (name + ".cfg")
            plain_cfg.write_text(cfg)
            declared_cfg = work / (name + "Declared.cfg")
            declared_cfg.write_text(cfg + "\nSYMMETRY " + symmetry + "\n")
            if refresh or name not in expected:
                ref = run_tlc(str(tla), ["-workers", "1", "-config", str(declared_cfg)], timeout=timeout)
                if ref.get("result") != "success":
                    print(f"FAIL {name} TLC: {ref}")
                    return 1
                expected[name] = signature(ref)
            for tier, flags, env in TIERS:
                got, out = run(binary, tla, plain_cfg, flags, env, timeout)
                problems = []
                if signature(got) != expected[name]:
                    problems.append("TLC counts/verdict")
                if "runtime guard" not in out or "(symmetry-reduced)" not in out:
                    problems.append("not guarded/reduced")
                if tier != "interpreter" and "actions lowered" not in out:
                    problems.append("compiler fallback")
                reports.append(record(name, tier, got, problems, out))
            got, out = run(binary, tla, declared_cfg, ["-compile", "-tlc"], {}, timeout)
            problems = [] if signature(got) == expected[name] else ["declared TLC parity"]
            reports.append(record(name, "declared-tlc", got, problems, out))
        name = "GuardLate"
        tla, cfg = work / (name + ".tla"), work / (name + ".cfg")
        shutil.copyfile(FIXTURES / tla.name, tla)
        shutil.copyfile(FIXTURES / cfg.name, cfg)
        for tier, flags, env in TIERS:
            ref, _ = run(binary, tla, cfg, flags + ["-noautosymmetry"], env, timeout)
            got, out = run(binary, tla, cfg, flags, env, timeout)
            problems = []
            if signature(ref) != signature(got) or ref.get("result") != "safety failure":
                problems.append("missed violation/counts/trace")
            if "restarting without automatic symmetry" not in out:
                problems.append("missing restart")
            if "(symmetry-reduced)" in out:
                problems.append("stale reduction label")
            reports.append(record(name, tier, got, problems, out))
    if refresh:
        cache_path.write_text(json.dumps(expected, indent=2, sort_keys=True) + "\n")
    if report_path:
        Path(report_path).write_text(json.dumps(reports, indent=2) + "\n")
    failures = sum(bool(r["problems"]) for r in reports)
    print(f"{len(reports)} guard fixture checks, {failures} failures")
    return int(bool(failures))


def run(binary, tla, cfg, flags, env, timeout):
    # Force the actual tier even when the reduced fixture fits in a pre-run.
    env = {"TLX_PRERUN": "1", "TLX_PRERUN_REUSE": "0", **env}
    cmd = [binary, "check", str(tla), "-config", str(cfg), *mode_args(binary, flags, mode="default")]
    p = run_proc(cmd, timeout, cwd=tla.parent, env=tlx_env(classpath=True, extra=env), grace=5)
    out = p.stdout + p.stderr
    return parse(out, 0, p.returncode), out


def record(name, tier, got, problems, out):
    print(f"{'FAIL' if problems else 'PASS'} {name}/{tier}: {signature(got)} {'; '.join(problems)}", flush=True)
    if problems:
        print(out[-3000:], flush=True)
    return {"model": name, "tier": tier, "got": signature(got), "problems": problems}
