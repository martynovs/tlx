"""Export and verify portable benchmark evidence without running model checks.

PYTHONPATH=scripts python3 -m benchmarking.snapshot create --out FILE.tar.gz
PYTHONPATH=scripts python3 -m benchmarking.snapshot verify FILE.tar.gz
"""
import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile
import tempfile
import tomllib

from benchmarking import bench_records, bench_suite
ROOT = Path(__file__).resolve().parents[2]
INPUTS = ('bench/README.md', 'bench/suite.json', 'bench/report-policy.json',
          'bench/repr-models.toml', 'bench/repr-configs/', 'bench/stress/', 'bench/scaling/')
DATA = ('bench/records/', 'bench/tlc-baselines/', 'bench/history/', 'bench/environment/')
REPORT = 'docs/performance.md'


def allowed(name):
    path = PurePosixPath(name)
    if path.is_absolute() or '..' in path.parts or str(path) != name:
        return False
    source = path.parts[0] == 'scripts' and path.suffix == '.py' and 'tests' not in path.parts
    return source or name == REPORT or any(name.startswith(p) if p.endswith('/') else name == p
                                 for p in (*INPUTS, *DATA))


def digest(data):
    return hashlib.sha256(data).hexdigest()


def check_inputs(root):
    root = Path(root)
    suite = json.loads((root / 'bench/suite.json').read_text())
    issues = bench_suite.validate_manifest(suite)
    policy = json.loads((root / 'bench/report-policy.json').read_text())
    if not isinstance(policy.get('ages'), dict) or not isinstance(policy.get('stale_after_revisions'), int):
        issues.append('invalid report policy')
    models = tomllib.loads((root / 'bench/repr-models.toml').read_text())
    for name, model in models.items():
        for key in ('cfg', 'wrapper'):
            if key in model and (not allowed(model[key]) or not (root / model[key]).is_file()):
                issues.append(f'{name}: missing or non-public {key}: {model[key]}')
    from benchmarking.models import LIVENESS_MODELS, MESSAGE_MODELS, SCALE_CFGS
    for name in (*LIVENESS_MODELS, *MESSAGE_MODELS, 'bench/ClockSettles', 'bench/FlexibleRaftReduced5'):
        if not (root / 'bench/stress' / (name.split('/')[-1] + '.cfg')).is_file():
            issues.append(f'{name}: missing stress configuration')
    for name in SCALE_CFGS:
        if not (root / 'bench/scaling' / (name.split('/')[-1] + '.cfg')).is_file():
            issues.append(f'{name}: missing scaling configuration')
    if not (root / 'bench/stress/FlexibleRaftReduced5.tla').is_file():
        issues.append('missing FlexibleRaftReduced5 wrapper')
    if issues:
        raise ValueError('; '.join(issues))
    return len(suite['models']), len(models)


def verify_report(root):
    root = Path(root)
    suite = json.loads((root / 'bench/suite.json').read_text())
    issues = bench_suite.validate_manifest(suite)
    if issues:
        raise ValueError('; '.join(issues))
    policy = json.loads((root / 'bench/report-policy.json').read_text())
    records = bench_records.load_records(root)
    if not records:
        raise ValueError('snapshot has no measurement records')
    rendered = bench_records.render(records, suite, stale_after=policy['stale_after_revisions'],
                                    ages=policy['ages'], baseline_inputs=policy.get('baseline_inputs'),
                                    report_note=policy.get('report_note'))
    if rendered != (root / REPORT).read_text():
        raise ValueError('snapshot measurements and policy do not reproduce docs/performance.md')
    return len(records)


def verify(path):
    """Verify content hashes and reproduce the report in a private directory."""
    with tarfile.open(path, 'r:gz') as archive, tempfile.TemporaryDirectory(prefix='tlx-bench-snapshot-') as d:
        members = archive.getmembers()
        names = [m.name for m in members]
        if len(set(names)) != len(names) or any(not m.isfile() for m in members):
            raise ValueError('snapshot must contain unique regular files only')
        if 'snapshot.json' not in names:
            raise ValueError('snapshot.json is missing')
        manifest = json.load(archive.extractfile('snapshot.json'))
        if manifest.get('version') != 1:
            raise ValueError('unsupported snapshot version')
        expected = manifest['files']
        if set(names) != set(expected) | {'snapshot.json'}:
            raise ValueError('snapshot file inventory differs from its manifest')
        for member in members:
            if member.name == 'snapshot.json':
                continue
            if not allowed(member.name):
                raise ValueError(f'unsupported snapshot path: {member.name}')
            data = archive.extractfile(member).read()
            if digest(data) != expected[member.name]:
                raise ValueError(f'checksum mismatch: {member.name}')
            target = Path(d, member.name)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        count = verify_report(d)
        if count != manifest['records']:
            raise ValueError('measurement count differs from the manifest')
        return count


def create(root, output):
    """Capture data once, verify the captured bytes, then publish without overwrite."""
    root, output = Path(root), Path(output)
    if output.exists() or output.with_name(output.name + '.sha256').exists():
        raise ValueError(f'output already exists: {output}')
    paths = {REPORT}
    paths.update(str(p.relative_to(root)) for p in (root / 'scripts').rglob('*.py')
                 if 'tests' not in p.relative_to(root).parts)
    for item in (*INPUTS, *DATA):
        p = root / item
        if p.is_dir():
            paths.update(str(f.relative_to(root)) for f in p.rglob('*') if f.is_file())
        elif p.is_file():
            paths.add(item)
    with tempfile.TemporaryDirectory(prefix='tlx-bench-capture-') as d:
        captured = Path(d)
        hashes = {}
        for name in sorted(paths):
            p = root / name
            if p.is_symlink() or not allowed(name):
                raise ValueError(f'unsupported source path: {name}')
            data = p.read_bytes()
            target = captured / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            hashes[name] = digest(data)
        count = verify_report(captured)
        git = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=root, capture_output=True, text=True)
        status = subprocess.run(['git', 'status', '--porcelain'], cwd=root, capture_output=True, text=True)
        metadata = {'version': 1, 'records': count, 'files': hashes,
                    'source_revision': git.stdout.strip() if git.returncode == 0 else None,
                    'source_dirty': bool(status.stdout) if status.returncode == 0 else None}
        data = (json.dumps(metadata, indent=2, sort_keys=True) + '\n').encode()
        candidate = captured / 'snapshot.tar.gz'
        with tarfile.open(candidate, 'w:gz') as archive:
            for name in sorted(paths):
                archive.add(captured / name, arcname=name, recursive=False)
            info = tarfile.TarInfo('snapshot.json')
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
        verify(candidate)
        output.parent.mkdir(parents=True, exist_ok=True)
        with output.open('xb') as target:
            target.write(candidate.read_bytes())
    checksum = digest(output.read_bytes())
    output.with_name(output.name + '.sha256').write_text(f'{checksum}  {output.name}\n')
    return count, checksum


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('inputs')
    export = commands.add_parser('create')
    export.add_argument('--root', type=Path, default=ROOT)
    export.add_argument('--out', type=Path, required=True)
    check = commands.add_parser('verify')
    check.add_argument('archive', type=Path)
    args = parser.parse_args()
    try:
        if args.command == 'inputs':
            models, reduced = check_inputs(ROOT)
            print(f'benchmark inputs valid: {models} suite models, {reduced} reduced configurations')
        elif args.command == 'create':
            count, checksum = create(args.root, args.out)
            print(f'{count} records exported; report reproduced; sha256 {checksum}')
        else:
            print(f'{verify(args.archive)} records verified; report reproduced')
    except (OSError, ValueError, KeyError, tarfile.TarError) as error:
        parser.exit(2, f'benchmark snapshot: {error}\n')


if __name__ == '__main__':
    main()
