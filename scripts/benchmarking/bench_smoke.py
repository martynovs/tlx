"""Prelaunch smoke checks through the requested benchmark's measurement path.

Fixtures deliberately cover cwd-sensitive scratch models, external include
paths and simulation. They use tiny state spaces and never write benchmark records.
"""

from benchmarking import models as bench_models, machine as bench_machine
import tempdirs

import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile

from tlxlib import ROOT


MEASURE_COMMANDS = {'run', 'ab', 'layout', 'table', 'tlcbase', 'scale', 'liveness', 'disk', 'profile', 'suite', 'ttfr', 'coverage'}


def preflight(commands, fresh=None):
    # Bring target/release/tlx up to date before the timed smoke runs: a rebuild
    # inside them hit their 60 s limit (2026-10-07).
    if fresh is None:
        fresh = lambda: bench_machine.require_fresh(bench_machine.TLX)
    # A census's new counters must compile in test targets too. Reject this
    # locally before a launch is reserved: a release-only build missed a
    # Clippy test-module error and consumed a census launch.
    if any(shlex.split(c)[:1] == ['coverage'] for c in commands):
        checked = subprocess.run(['cargo', 'clippy', '--workspace', '--release', '--all-targets', '-j', '3', '--', '-D', 'warnings'],
                                 cwd=ROOT, capture_output=True, text=True, timeout=600)
        if checked.returncode:
            raise ValueError(f'census prelaunch Clippy failed:\n{checked.stdout}\n{checked.stderr}')
    fresh()
    for command in commands:
        words = shlex.split(command)
        if not words:
            raise ValueError('empty --bench command')
        if words[0] not in MEASURE_COMMANDS:
            raise ValueError(f'--bench smoke does not support {words[0]!r}; use a measurement subcommand')
        result = subprocess.run([sys.executable, '-u', os.path.join(ROOT, 'scripts/bench.py'), *words, '--smoke'],
                                cwd=ROOT, capture_output=True, text=True, timeout=60)
        if result.returncode:
            raise ValueError(f'bench smoke failed ({command}):\n{result.stdout}\n{result.stderr}')
        print(result.stdout, end='', flush=True)


def run(args):
    import bench
    from benchmarking import bench_disk
    from benchmarking import bench_profile
    if args.cmd == 'layout' and args.models:
        keys = [k.strip() for k in args.models.split(',') if k.strip()]
        models = bench_models.oracle(keys)
        for key in keys:
            if key not in models:
                raise ValueError(f'unknown model {key}')
    binary = os.path.join(ROOT, 'target/release/tlx')
    if not os.access(binary, os.X_OK):
        raise ValueError('bench smoke needs target/release/tlx; run cargo build --release first')
    with tempdirs.TemporaryDirectory(prefix='tlx-bench-smoke-') as directory:
        for kind in ('scratch', 'external', 'simulation'):
            root = Path(directory) / kind / 'model with spaces'
            root.mkdir(parents=True)
            (root / 'input.json').write_text('[1, 2]')
            (root / 'Smoke.tla').write_text('''---- MODULE Smoke ----
EXTENDS Naturals, Json
ASSUME JsonDeserialize("input.json") = <<1, 2>>
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
====
''')
            config = root.parent / 'Smoke.cfg' if kind == 'external' else root / 'Smoke.cfg'
            config.write_text('INIT Init\nNEXT Next\n')
            model = {'id': 'smoke/' + kind, 'tla': str(root / 'Smoke.tla'), 'cfg': str(config),
                     'scratch': True, 'flags': [], 'features': [], 'estimate_s': 1}
            flags = shlex.split(getattr(args, 'args', '-compile'))
            if kind == 'simulation':
                flags += ['-simulate', 'num=1', '-depth', '2']
            if args.cmd == 'coverage':
                import coverage
                model['flags'] = flags[1:] if flags[:1] == ['-compile'] else flags
                census = coverage.run(model, 10, binary, True)
                if census.get('dynamic', {}).get('native_share') is None:
                    raise ValueError(f"coverage {kind} smoke has no dynamic native-share measurement")
                result = {'result': census['status'], 'wall_s': census['wall_s']}
            elif args.cmd == 'disk':
                result = bench_disk.run_tlx(model, binary, flags, 10, {})
            elif args.cmd == 'profile':
                result = bench_profile.run_one(model, flags, 10, binary)
            else:
                result = bench.tlx(model, flags, 10, exe=binary)
            if result.get('result') != 'success':
                raise ValueError(f"{args.cmd} {kind} smoke failed: {result}")
            print(f"PASS smoke {args.cmd} {kind}: {result['wall_s']:.3f}s", flush=True)
    return 0
