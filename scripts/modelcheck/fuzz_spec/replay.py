"""Recording and replaying saved state-spec oracle bundles."""
import json
import os
import shutil
import sys
import tempdirs
from modelcheck.results import must
from tlxlib import CaseResult
from . import artifacts
from .artifacts import normalize
from .judgment import brief
from .render import spec_files
from .runner import mode_variant, run_tlc, run_tlx


def record(names):
    """Run TLC on saved regressions (in a scratch copy) and store its output."""
    for name in names:
        if not os.path.exists(os.path.join(artifacts.SPECS, name + ".tla")):
            print(f"unknown regression {name}", file=sys.stderr)
            return 2
        with tempdirs.TemporaryDirectory(prefix="tlx-fuzz-record-") as d:
            for f in spec_files(artifacts.SPECS, name):
                shutil.copy2(os.path.join(artifacts.SPECS, f), d)
            out, rc, _ = run_tlc(d, name)
        if out is None:
            print(f"{name}: TLC timed out", file=sys.stderr)
            return 1
        os.makedirs(os.path.join(artifacts.SPECS, "tlc"), exist_ok=True)
        with open(os.path.join(artifacts.SPECS, "tlc", name + ".out"), "w") as f:
            f.write(normalize(out, d, name))
        print(f"{name}: {brief(must(out, rc))}")
    return 0


def replay(runner, names, modes, scratch):
    """[CaseResult] of the saved regressions in `modes`."""
    cases = []
    validated = {}
    if os.path.exists(artifacts.VALIDATED):
        with open(artifacts.VALIDATED) as f:
            validated = json.load(f)
    runner.validated = validated
    for name in names:
        d = os.path.join(scratch, "replay", name)
        os.makedirs(d, exist_ok=True)
        for f in spec_files(artifacts.SPECS, name):
            shutil.copy2(os.path.join(artifacts.SPECS, f), d)
        with open(os.path.join(artifacts.SPECS, "tlc", name + ".out")) as f:
            out = f.read()
        want = must(out, None)
        key = name
        if os.path.exists(os.path.join(artifacts.SPECS, name + ".json")):
            with open(os.path.join(artifacts.SPECS, name + ".json")) as f:
                key = json.load(f).get("variant", name)
        for mode in modes:
            got = run_tlx(runner.binary, d, name, mode, 120, want.get("distinct"), key=key)
            diffs, note = runner.compare(d, name, want, got, mode_variant(key, mode)[2])
            cases.append(CaseResult(f"replay/{name}/{mode}", "fail" if diffs else "pass",
                                    {"kind": got.get("result"), "differs": diffs, "note": note, "tlc": brief(want), "tlx": brief(got)}))
            print(f"{'FAIL' if diffs else 'PASS'} replay/{name}/{mode} tlc={brief(want)} tlx={brief(got)}"
                  + (f" differs: {diffs}" if diffs else "") + (f" ({note[:200]})" if note else ""), flush=True)
    if validated:
        os.makedirs(os.path.dirname(artifacts.VALIDATED), exist_ok=True)
        with open(artifacts.VALIDATED, "w") as f:
            json.dump(validated, f, indent=1, sort_keys=True)
            f.write("\n")
    runner.validated = {}
    return cases
