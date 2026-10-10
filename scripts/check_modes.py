"""Lint tlx command construction for an explicit tlxlib execution-mode choice."""
import ast
from pathlib import Path

HELPERS = {'interp_args', 'parity_args', 'mode_args'}


def names_tlx(node):
    """Is the first element of this command literal the tlx binary?

    A variable (or any other expression) may hold the binary, so only a string
    literal can rule it out: `["cargo", "check", ...]` is not a tlx invocation
    (2026-10-07: the feature-matrix gate step tripped the lint this way).
    """
    if not node.elts:
        return False
    first = node.elts[0]
    if isinstance(first, ast.Constant):
        return isinstance(first.value, str) and first.value.rsplit('/', 1)[-1] in {'tlx', 'tlx.exe'}
    return True


def findings(path, source):
    tree = ast.parse(source, filename=str(path))
    helpers, modules = set(), set()
    for node in ast.walk(tree):
        if isinstance(node, ast.ImportFrom) and node.module == 'tlxlib':
            helpers.update(alias.asname or alias.name for alias in node.names if alias.name in HELPERS)
        if isinstance(node, ast.Import):
            modules.update(alias.asname or alias.name for alias in node.names if alias.name == 'tlxlib')
    problems = []
    for function in ast.walk(tree):
        if not isinstance(function, (ast.FunctionDef, ast.AsyncFunctionDef)):
            continue
        definitions = {}
        for node in ast.walk(function):
            if isinstance(node, ast.Assign):
                for target in node.targets:
                    if isinstance(target, ast.Name):
                        definitions.setdefault(target.id, []).append(node.value)
        def explicit(node, seen=frozenset()):
            if isinstance(node, ast.Call):
                fn = node.func
                if isinstance(fn, ast.Name) and fn.id in helpers:
                    return True
                if isinstance(fn, ast.Attribute) and isinstance(fn.value, ast.Name) and fn.value.id in modules and fn.attr in HELPERS:
                    return True
            if isinstance(node, ast.Name) and node.id not in seen:
                if any(explicit(value, seen | {node.id}) for value in definitions.get(node.id, [])):
                    return True
            return any(explicit(child, seen) for child in ast.iter_child_nodes(node))
        for node in ast.walk(function):
            if (isinstance(node, (ast.List, ast.Tuple)) and len(node.elts) > 1
                    and isinstance(node.elts[1], ast.Constant) and node.elts[1].value == 'check'
                    and names_tlx(node) and not explicit(node)):
                problems.append(f'{path}:{node.lineno}: tlx check needs tlxlib interp_args/parity_args/mode_args')
    return sorted(set(problems))


def sources(root):
    """Every harness source of this tree, nested packages included (HARNESS-13).

    A flat `scripts/*.py` glob silently omits a module once it moves into
    scripts/<package>/: later cleanup batches move modules there, so the lint
    has to see them before that happens.
    """
    return [path for path in sorted((Path(root) / 'scripts').rglob('*.py'))
            if not path.name.startswith('test_')]


def check(root):
    return [problem for path in sources(root) for problem in findings(path, path.read_text())]


if __name__ == '__main__':
    problems = check(Path(__file__).resolve().parents[1])
    print('\n'.join(problems) if problems else 'check_modes: 0 bare tlx check invocations')
    raise SystemExit(bool(problems))
