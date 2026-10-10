#!/usr/bin/env python3
"""Check the registered environment names in source and scripts (or the index)."""
import argparse
import json
from pathlib import Path
import re
from harness.git import run as run_git

ROOT = Path(__file__).resolve().parents[1]
REGISTRY = "crates/tlx-base/src/knobs.rs"
ALLOWLIST = "scripts/knobs_allowlist.json"
NAME = re.compile(r"\bTLX_[A-Z0-9_]+\b")
DECLARATION = re.compile(r'(?:Knob::new|flag|flag_on|num|opt_num|text)\(\s*"(TLX_[A-Z0-9_]+)"')


def git(root, *args):
    return run_git(*args, cwd=root, text=False, check=True).stdout


def index_texts(root, names):
    """Read index blobs in one process, including filenames containing whitespace."""
    refs = [":" + name for name in names]
    data = run_git("cat-file", "--batch", cwd=root, input=("\n".join(refs) + "\n").encode(),
                   text=False, check=True).stdout
    out, offset = {}, 0
    for name in names:
        end = data.index(b"\n", offset)
        header = data[offset:end].split()
        if header[-1] == b"missing":
            raise ValueError(f"missing staged file: {name}")
        size = int(header[-1])
        blob = data[end + 1:end + 1 + size]
        offset = end + size + 2
        try:
            out[name] = blob.decode("utf-8")
        except UnicodeDecodeError:
            pass
    return out


def check(root=ROOT, staged=False):
    root = Path(root)
    names = git(root, "ls-files", "-z", "--", "crates", "scripts").decode().split("\0")
    names = [name for name in names if name and (name.endswith((".rs", ".py", ".sh")) or name == ALLOWLIST)]
    if staged:
        texts = index_texts(root, names)
    else:
        texts = {}
        for name in names:
            try:
                texts[name] = (root / name).read_text()
            except (UnicodeDecodeError, FileNotFoundError):
                continue
    if REGISTRY not in texts:
        return [f"missing knob registry: {REGISTRY}"]
    # Only declaration rows count, never examples/unknown-name tests in the registry.
    registered = set(DECLARATION.findall(texts[REGISTRY]))
    allowed = json.loads(texts.get(ALLOWLIST, "{}"))
    problems = []
    for name, text in texts.items():
        if name == ALLOWLIST or not name.endswith((".rs", ".py", ".sh")):
            continue
        exemptions = {**allowed.get("_symbols", {}), **allowed.get(name, {})}
        unknown = set(NAME.findall(text)) - registered - exemptions.keys()
        if not unknown:
            continue
        for lineno, line in enumerate(text.splitlines(), 1):
            for knob in sorted(set(NAME.findall(line)) & unknown):
                problems.append(f"{name}:{lineno}: {knob} is not registered in {REGISTRY}; "
                                "the TLX_ prefix is reserved for registered environment knobs. "
                                "Rename other identifiers (including TLA+ constants); register real knobs in the registry and docs/configuration.md")
    return problems


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--staged", action="store_true", help="check the exact Git index")
    a = ap.parse_args()
    problems = check(staged=a.staged)
    print("\n".join(problems) if problems else "knobs: all source names are registered")
    return bool(problems)


if __name__ == "__main__":
    raise SystemExit(main())
