#!/usr/bin/env python3
"""Reconcile Rust unsafe-containing files with the register in docs/unsafe-audit.md.

Scan every crates/**/*.rs, including untracked additions, tests, build scripts and
inactive cfg branches. This is lexical file coverage, not a Rust safety proof:
macro bodies count, but comments, literals and raw identifiers do not. Generated C,
dependencies and generated OUT_DIR artifacts are outside this source-file boundary.
"""
import argparse
from pathlib import Path
import re

BEGIN = '<!-- unsafe-inventory:start -->'
END = '<!-- unsafe-inventory:end -->'
AUDIT = 'docs/unsafe-audit.md'
TOKEN = re.compile(
    r'(?P<line>//[^\n]*)|(?P<block>/\*)|'
    r'(?P<raw>(?:br|cr|r)(?P<hashes>\#*)")|'
    r'(?P<string>(?:b|c)?"(?:\\[\s\S]|[^"\\])*")|'
    r"(?P<char>b?'(?:\\(?:u\{[\da-fA-F_]+\}|x[\da-fA-F]{2}|.)|[^'\\\n])')|"
    r'(?P<raw_id>r\#\w+)|(?P<ident>[^\W\d]\w*)|(?P<other>[\s\S])')
ROW = re.compile(r'^\| `(?P<path>crates/[A-Za-z0-9_./-]+\.rs)` \| '
                 r'(?P<status>reviewed|excluded) \| (?P<reason>[^|]+) \|$')


def unsafe_lines(source):
    """Return 1-based lines with unsafe keyword tokens (nested comments supported)."""
    found, pos, line = [], 0, 1
    while pos < len(source):
        token = TOKEN.match(source, pos)
        end = token.end()
        if token.lastgroup == 'block':
            depth = 1
            while depth and end < len(source):
                if source.startswith('/*', end):
                    depth += 1
                    end += 2
                elif source.startswith('*/', end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
        elif token.group('raw') is not None:
            closing = '"' + token.group('hashes')
            closing_at = source.find(closing, end)
            end = len(source) if closing_at < 0 else closing_at + len(closing)
        elif token.lastgroup in ('ident', 'raw_id'):
            # Python's \w omits combining marks legal in Rust identifiers.
            # Do not mistake the prefix of such an identifier for a keyword.
            while end < len(source) and ('a' + source[end]).isidentifier():
                end += 1
            if source[pos:end] == 'unsafe' and (not found or found[-1] != line):
                found.append(line)
        line += source.count('\n', pos, end)
        pos = end
    return found


def check(root):
    """Return diagnostics and explicit file counts, without printing or changing cwd."""
    root = Path(root)
    files = sorted((root / 'crates').rglob('*.rs'))
    actual = {}
    for path in files:
        lines = unsafe_lines(path.read_text(encoding='utf-8'))
        if lines:
            actual[path.relative_to(root).as_posix()] = lines
    counts = dict(scanned=len(files), unsafe=len(actual), reviewed=0, excluded=0)
    audit = root / AUDIT
    problems, registered = [], {}
    if not files:
        problems.append('unsafe inventory: no Rust source files under crates/ (check the repository root)')
    if not audit.is_file():
        # Minimal doccheck fixtures without any unsafe source need no register.
        return problems + ([f'{AUDIT}: missing register'] if actual else []), counts
    text = audit.read_text(encoding='utf-8')
    if text.count(BEGIN) != 1 or text.count(END) != 1 or text.index(BEGIN) >= text.index(END):
        return [f'{AUDIT}: require exactly one ordered inventory marker pair'], counts
    body = text.split(BEGIN, 1)[1].split(END, 1)[0]
    for line in body.splitlines():
        if not line.strip() or line in ('| File | Disposition | Review / exclusion reason |', '|---|---|---|'):
            continue
        match = ROW.fullmatch(line)
        if not match or not match['reason'].strip():
            problems.append(f'{AUDIT}: invalid inventory row (explicit path, disposition and reason required): {line}')
            continue
        path, status = match['path'], match['status']
        if '..' in Path(path).parts or Path(path).as_posix() != path:
            problems.append(f'{AUDIT}: noncanonical inventory path: {path}')
        elif path in registered:
            problems.append(f'{AUDIT}: duplicate inventory file: {path}')
        else:
            registered[path] = status
            counts[status] += 1
    for path in sorted(actual.keys() - registered.keys()):
        problems.append(f'{path}:{actual[path][0]}: unregistered unsafe; review and add an explicit inventory row')
    for path in sorted(registered.keys() - actual.keys()):
        problems.append(f'{AUDIT}: stale inventory file (missing or no unsafe tokens): {path}')
    return problems, counts


def summary(counts):
    return 'unsafe inventory: ' + ' '.join(f'{key}={value}' for key, value in counts.items())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', nargs='?', default='.')
    args = parser.parse_args()
    problems, counts = check(args.root)
    print(summary(counts))
    for problem in problems:
        print(problem)
    return bool(problems)


if __name__ == '__main__':
    raise SystemExit(main())
