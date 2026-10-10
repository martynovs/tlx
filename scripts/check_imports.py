#!/usr/bin/env python3
"""Import direction under `scripts/`: no cycle outside a declared cluster.

HARNESS-03: reusable domain libraries stay independent of command entrypoints
and test modules, because an import back into an executed entrypoint creates
cycles, duplicate module identities (`scripts/bench.py` publishes itself under
its import name for exactly that reason) and hidden global state, while an
import of a test module reverses the production/test direction the audit
(`benchmarking.bench_suite.commit_range` importing a policy from `test_no_task_ids`) found.

Two rules, both read from the whole AST with function bodies included: the
repository defers its back-edges into function bodies on purpose (the
`benchmarking/commands/` passes reach `bench.py`'s shared helpers that way), so
a check that reads only the module body sees none of them.

1. A non-test module does not import a test module.
2. The import graph has no cycle outside `ACCEPTED_CYCLES`.

Rule 2 answers a literal "a library must not import its CLI" without having to
decide which of the ~190 modules are libraries: the harm HARNESS-03 names
happens only when the import is returned, and a returned import is a cycle. An
import of an entrypoint that nothing returns is a plain downward edge and stays
allowed, which is what lets `corpus` and `tlc` be read as libraries by the
passes that need their metadata.

`ACCEPTED_CYCLES` declares each cycle that exists today as one whole cluster:
the module that owns the temporary wrapper, the members, why the cycle is
tolerated and what ends it. Accepting a cluster as a whole is deliberate -- a
module that joins one (a new back-edge into `bench.py`) changes its member set
and fails, so the declaration cannot silently shelter the next wrapper -- and a
cluster that no longer cycles is a stale entry that also fails.
"""
import argparse
import ast
import sys
from pathlib import Path

from harness.git import run as run_git

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = "scripts"

#: The cyclic import clusters under `scripts/` that exist today. Each is one
#: temporary wrapper, named by the module that owns it, with what ends the
#: exemption so the entry cannot outlive the migration it was written for.
ACCEPTED_CYCLES = (
    {
        "owner": "bench",
        "reason":
            "the bench program: `scripts/bench.py` is the namespace the subcommand passes in"
            " `benchmarking/commands/` reach the shared helpers through (`bench.tlx`,"
            " `bench.mode_args`) and the name a test replaces one at, while `bench_profile.py`,"
            " `bench_suite.py`, `bench_smoke.py`, `bench_ttfr.py`, `bench_disk.py`, `coverage.py`"
            " and `benchmarking/cli.py` are still read from the same module",
        "ends":
            "when the shared helpers gain a library owner and `scripts/bench.py` deletes its"
            " re-exports, so no pass, driver or test reaches the entrypoint",
        "members": frozenset({
            "bench", "benchmarking.bench_disk", "benchmarking.bench_profile", "benchmarking.bench_smoke", "benchmarking.bench_suite", "benchmarking.bench_ttfr",
            "benchmarking.cli", "benchmarking.commands.ab", "benchmarking.commands.history",
            "benchmarking.commands.layout", "benchmarking.commands.run",
            "benchmarking.commands.scale", "benchmarking.commands.tlcbase", "coverage",
        }),
    },
    {
        "owner": "cloud.session",
        "reason":
            "`cloud/session.py` runs the phases of `cloud/remote.py` through the `Hooks` object"
            " the entrypoint passes in, and `remote.py` imports the session's `Session` type for"
            " its annotations",
        "ends":
            "when the phase protocol lives in a module of its own that neither side imports back"
            " (only the type annotation keeps the return edge alive)",
        "members": frozenset({"cloud.remote", "cloud.session"}),
    },
    {
        "owner": "corpus",
        "reason":
            "the corpus manifest: `scripts/corpus.py` owns the model list and `scripts/oracle.py`"
            " reads it, while `corpus.py` asks `oracle.py` for the oracle a model needs",
        "ends":
            "when the manifest moves into a library (the same owner `benchmarking/` reads it"
            " through) and `corpus.py` keeps only the command line",
        "members": frozenset({"corpus", "oracle"}),
    },
)


def module_name(name):
    """The name `scripts/<name>` is imported under, or None when it is not a module."""
    path = Path(name)
    if path.suffix != ".py":
        return None
    parts = list(path.with_suffix("").parts)
    if parts and parts[0] == PACKAGE:
        parts = parts[1:]
    if not parts or parts[-1] == "__main__":
        return None
    if parts[-1] == "__init__":
        parts = parts[:-1]
    return ".".join(parts) if parts else None


def imported_names(tree):
    """Every module a source imports as `(written name, line)`, function bodies included."""
    found = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            found += [(alias.name, node.lineno) for alias in node.names]
        elif isinstance(node, ast.ImportFrom):
            if node.level or not node.module:
                continue  # a relative import stays inside the importer's package
            for alias in node.names:
                found.append((node.module + "." + alias.name, node.lineno))  # `from pkg import mod`
            found.append((node.module, node.lineno))
    return found


def resolve(name, known):
    """The longest module prefix of `name` that exists, or None."""
    parts = name.split(".")
    while parts:
        candidate = ".".join(parts)
        if candidate in known:
            return candidate
        parts.pop()
    return None


def modules(names, read):
    """`{module name: (path, source)}` for every Python module in `names`."""
    known = {}
    for name in sorted(names):
        dotted = module_name(name)
        if dotted is None:
            continue
        try:
            known[dotted] = (name, read(name))
        except (UnicodeDecodeError, FileNotFoundError, KeyError):
            continue
    return known


def program(tree):
    """Whether a source is written to be executed as a program."""
    return any(isinstance(node, ast.If) and "__main__" in ast.dump(node.test) for node in tree.body)


def graph(sources):
    """`{module: {imported module: line}}` and `{module: is a program}`, tests included."""
    edges, programs = {}, set()
    for name, (_, text) in sources.items():
        try:
            tree = ast.parse(text)
        except SyntaxError:
            continue
        if program(tree):
            programs.add(name)
        out = edges.setdefault(name, {})
        for written, lineno in imported_names(tree):
            target = resolve(written, sources)
            if target is not None and target != name:
                out.setdefault(target, lineno)
    return edges, programs


def certificates(edges):
    """The strongly connected components with more than one module (Tarjan)."""
    index, low, stack, on_stack, components, counter = {}, {}, [], set(), [], [0]

    def visit(node):
        index[node] = low[node] = counter[0]
        counter[0] += 1
        stack.append(node)
        on_stack.add(node)
        for neighbour in sorted(edges.get(node, ())):
            if neighbour not in index:
                visit(neighbour)
                low[node] = min(low[node], low[neighbour])
            elif neighbour in on_stack:
                low[node] = min(low[node], index[neighbour])
        if low[node] == index[node]:
            component = []
            while True:
                member = stack.pop()
                on_stack.discard(member)
                component.append(member)
                if member == node:
                    break
            if len(component) > 1:
                components.append(frozenset(component))

    for node in sorted(edges):
        if node not in index:
            visit(node)
    return components


def cycle_through(edges, importer, target):
    """A shortest path `target -> ... -> importer`, or `target` alone when there is none."""
    seen, queue = {target: None}, [target]
    while queue:
        node = queue.pop(0)
        for neighbour in sorted(edges.get(node, ())):
            if neighbour in seen:
                continue
            seen[neighbour] = node
            if neighbour == importer:
                path, walk = [], importer
                while walk is not None:
                    path.append(walk)
                    walk = seen[walk]
                return list(reversed(path))
            queue.append(neighbour)
    return [target]


def problems_in(sources):
    """The two rules' problems in one module set, one line each."""
    sys.setrecursionlimit(max(sys.getrecursionlimit(), 4 * len(sources) + 100))  # certificates() recurses
    edges, programs = graph(sources)
    path_of = {name: path for name, (path, _) in sources.items()}
    problems = []
    for importer in sorted(edges):
        for target, lineno in sorted(edges[importer].items(), key=lambda item: item[1]):
            if Path(path_of[target]).name.startswith("test_") and \
                    not Path(path_of[importer]).name.startswith("test_"):
                problems.append(
                    f"{path_of[importer]}:{lineno}: {importer} imports the test module {target};"
                    " production must not depend on a test module (HARNESS-03) -- the policy or"
                    " helper it needs belongs to a library, not to a test")
    declared = [set(cycle["members"]) for cycle in ACCEPTED_CYCLES]
    used = set()
    for component in sorted(certificates(edges), key=sorted):
        for index, members in enumerate(declared):
            if members == set(component):
                used.add(index)
                break
        else:
            # Report the edge the cycle returns on: the import of the module that
            # is itself a program, so the message names the entrypoint involved.
            candidates = sorted((importer, target, line) for importer in component
                                for target, line in edges[importer].items() if target in component)
            importer, target, lineno = next(
                (edge for edge in candidates if edge[1] in programs), candidates[0])
            shown = " -> ".join([importer, *cycle_through(edges, importer, target)])
            problems.append(
                f"{path_of[importer]}:{lineno}: {importer} imports {target}, closing the undeclared"
                f" import cycle {shown} (members: {', '.join(sorted(component))}); a cycle is a"
                " returned import, i.e. a library importing its command entrypoint (HARNESS-03: a"
                " returned import is a second module identity) -- break one of the imports, or"
                " declare the cluster with its owner and what ends it in scripts/check_imports.py"
                " ACCEPTED_CYCLES")
    for index, cycle in enumerate(ACCEPTED_CYCLES):
        if index in used or not set(cycle["members"]) & set(sources):
            # A tree that does not hold the cluster at all -- a fixture
            # checkout, a sparse export -- says nothing about it: those modules
            # are absent, not uncoupled.
            continue
        problems.append(
            f"scripts/check_imports.py: stale cycle declaration for {cycle['owner']}:"
            f" {', '.join(sorted(cycle['members']))} is no longer one import cycle, so delete"
            " the declaration (it excuses a wrapper that is gone)")
    return problems


def check(root=ROOT, staged=False):
    """The rules' problems in the `scripts/` tree at `root`, one line each."""
    listing = run_git("ls-files", "-z", "--", PACKAGE, cwd=root, text=False, check=True).stdout
    names = [name for name in (part.decode() for part in listing.split(b"\0")) if name and name.endswith(".py")]
    if staged:
        from knobs_check import index_texts
        texts = index_texts(root, names)
        read = texts.__getitem__
    else:
        read = lambda name: (Path(root) / name).read_text()  # noqa: E731
    return problems_in(modules(names, read))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--staged", action="store_true", help="check the exact Git index")
    ap.add_argument("--root", default=str(ROOT), help="repository root (the tests point it at a fixture)")
    args = ap.parse_args()
    problems = check(root=Path(args.root), staged=args.staged)
    print("\n".join(problems) if problems else
          "imports: no cycle outside the declared clusters, no production module imports a test")
    return bool(problems)


if __name__ == "__main__":
    raise SystemExit(main())
