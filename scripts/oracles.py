#!/usr/bin/env python3
"""Validated, direction-neutral oracle merge; CLI refresh writes worker caches only."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import subprocess

from harness.storage import atomic_json_replace, file_lock

HINT = 'refresh shared oracles with: sh harness/agent_setup.sh --refresh-oracles'
_MISSING = object()


def stamp(row):
    provenance = row.get('provenance') if isinstance(row, dict) else None
    value = provenance.get('date') if isinstance(provenance, dict) else None
    try:
        parsed = datetime.datetime.fromisoformat(value)
        return parsed.replace(tzinfo=datetime.timezone.utc) if parsed.tzinfo is None else parsed
    except (TypeError, ValueError):
        return None


def valid_cfg(row, root):
    if not isinstance(row, dict) or not isinstance(row.get('provenance'), dict):
        return None
    digest = row['provenance'].get('cfg_digest')
    name = row.get('cfg')
    if not isinstance(name, str) or not digest:
        return None
    marker = 'third_party/'
    if marker in name:
        name = name[name.index(marker):]
    path = Path(root) / name
    # Old cloud paths are rebased above; other absolute/escaping paths are not authority.
    if Path(name).is_absolute() or '..' in Path(name).parts:
        return None
    try:
        return hashlib.sha256(path.read_bytes()).hexdigest() == digest
    except OSError:
        return None


def validation_issue(row, root):
    """Conservatively retain legacy/unknown schemas; do not invent their provenance."""
    if not isinstance(row, dict) or not isinstance(row.get('tlc'), dict):
        return 'unvalidated'
    p, result = row.get('provenance'), row['tlc']
    if (not isinstance(p, dict) or stamp(row) is None or not p.get('tlc_version')
            or p.get('tlc_version') == 'unknown' or type(p.get('workers')) is not int or p['workers'] < 1
            or result.get('result') not in {'success', 'error', 'safety failure', 'deadlock',
                                          'property failure', 'init property failure',
                                          'liveness failure', 'postcondition failure'}
            or type(result.get('exit')) is not int or result['exit'] < 0
            or (result['result'] == 'success' and result['exit'] != 0)):
        return 'unvalidated'
    cfg = valid_cfg(row, root)
    return None if cfg is True else 'config-mismatch' if cfg is False else 'config-unavailable'


def decision(incoming, local, root):
    if incoming == local:
        return 'same'
    issue = validation_issue(incoming, root)
    if issue:
        return issue
    if local is _MISSING:
        return 'added'
    if not isinstance(local, dict) or not isinstance(local.get('provenance'), dict):
        return 'unknown-age'
    p, q = incoming['provenance'], local['provenance']
    if (any(p.get(k) != q.get(k) for k in ('cfg_digest', 'workers', 'tlc_version'))
            or any(incoming.get(k) != local.get(k) for k in ('id', 'flags', 'asymmetric_symmetry'))):
        return 'incompatible'
    # All result evidence except diagnostic/timing/trace-shape fields must agree.
    # This is deliberately stricter than runtime parity: uncertain conflicts need review.
    ignored = {'wall_s', 'cpu_s', 'error', 'tail', 'trace', 'lasso', 'exit'}
    def evidence(row):
        result = row.get('tlc')
        if not isinstance(result, dict):
            return None
        value = {k: v for k, v in result.items() if k not in ignored}
        if 'trace' in result:
            value['trace_length'] = len(result['trace'])
        return value
    if evidence(incoming) != evidence(local):
        return 'conflict'
    a, b = stamp(incoming), stamp(local)
    if a is None or b is None:
        return 'unknown-age'
    return 'updated' if a > b else 'older' if a < b else 'same-age'


def newer(incoming, local, root):
    return decision(incoming, local, root) == 'updated'


def merge_rows(incoming, local, root):
    """Return a merged object and per-key decisions, without mutating either input.

    Callers own direction and preservation. Rejected and displaced rows remain in
    their original source; cleanup adapters must retain both sides before calling.
    """
    if not isinstance(incoming, dict) or not isinstance(local, dict):
        raise ValueError('oracle cache must be an object')
    merged, decisions = dict(local), []
    for key, row in incoming.items():
        reason = decision(row, local[key] if key in local else _MISSING, root)
        if reason in ('added', 'updated'):
            merged[key] = row
        decisions.append(dict(key=key, decision=reason))
    return merged, decisions


def merge_file(source, destination, *, root, preserve=None):
    """Merge either direction under the destination's stable sidecar lock.

    An optional preservation callback receives both original byte streams before
    parsing or replacement. Its failure forbids publication, including corrupt input.
    """
    source, destination = Path(source), Path(destination)
    if destination.is_symlink() or source.resolve() == destination.resolve():
        raise ValueError('refusing a shared oracle symlink or source/destination alias')
    with file_lock(str(destination) + '.lock'):
        incoming = source.read_bytes()
        current = destination.read_bytes() if destination.exists() else None
        if preserve is not None:
            preserve(incoming, current)
        merged, decisions = merge_rows(json.loads(incoming), json.loads(current) if current is not None else {}, root)
        if current is None or merged != json.loads(current):
            atomic_json_replace(destination, merged)
    return decisions


def main_cache():
    """Git's common directory identifies main even when this script runs in a worker."""
    result = subprocess.run(['git', 'rev-parse', '--path-format=absolute', '--git-common-dir'],
                            cwd=Path(__file__).resolve().parent.parent, capture_output=True, text=True,
                            timeout=30, check=True)
    return Path(result.stdout.strip()).parent / 'corpus/out'


def sync(source, destination):
    source, destination = Path(source), Path(destination)
    if source.resolve() == destination.resolve() or destination.resolve() == main_cache().resolve():
        raise ValueError('oracle refresh must write a private worktree cache, never the main cache')
    if not source.is_dir():
        raise ValueError('oracle source directory is missing: '+str(source))
    added, updated, kept = 0, 0, 0
    retained = {}
    for path in sorted(source.glob('tlc*.json')):
        rows = merge_file(path, destination / path.name, root=destination.parent.parent)
        added += sum(row['decision'] == 'added' for row in rows)
        updated += sum(row['decision'] == 'updated' for row in rows)
        for row in rows:
            reason = row['decision']
            if reason not in ('added', 'updated'):
                kept += 1
                retained[reason] = retained.get(reason, 0) + 1
    print(f'oracles: {added} added, {updated} updated, {kept} retained', flush=True)
    if retained:
        print('oracles: retained at source/destination: '+', '.join(f'{k}={v}' for k, v in sorted(retained.items())), flush=True)
    return added, updated, kept


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['sync'])
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--destination', type=Path, default=Path('corpus/out'))
    args = parser.parse_args()
    sync(args.source, args.destination)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
