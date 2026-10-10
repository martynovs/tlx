"""Validate unique, increasing compatibility entry IDs and their references."""
import re
from pathlib import Path

ENTRY = re.compile(r'^\s*\|\s*D-(\d+)\s*\|', re.M)
REFERENCE = re.compile(r'\bD-(\d+)\b')


def check(text, references=()):
    problems, seen, previous = [], set(), 0
    for match in ENTRY.finditer(text):
        number = int(match.group(1))
        line = text.count('\n', 0, match.start()) + 1
        if number in seen:
            problems.append(f'docs/compat.md:{line}: duplicate D-{number}')
        if number <= previous:
            problems.append(f'docs/compat.md:{line}: out-of-order D-{number} after D-{previous}')
        seen.add(number)
        previous = number
    for path, contents in [('docs/compat.md', text), *references]:
        for line, contents in enumerate(contents.splitlines(), 1):
            if 'doccheck: ignore' in contents:
                continue
            for match in REFERENCE.finditer(contents):
                if int(match.group(1)) not in seen:
                    problems.append(f'{path}:{line}: reference to nonexistent D-{match.group(1)}')
    return problems


def check_files(paths):
    compat = Path('docs/compat.md')
    if not compat.exists():
        return []
    return check(compat.read_text(), [(p, Path(p).read_text(errors='replace')) for p in paths if p != str(compat)])
