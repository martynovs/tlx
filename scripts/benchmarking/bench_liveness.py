#!/usr/bin/env python3
"""Bounded, interleaved ClockSettles scaling benchmark (bench.py liveness).

Scratch TLA+ modules vary the clock's domain and recurrence conjunction
size together. A small Rust probe compiles each revision's actual tableau
source to measure automaton construction separately from model exploration.
The probe imports the release build's shared hash interner unchanged.
Incomplete runs are recorded as censored observations, never as speedups.
"""

if __package__ in (None, ""):
    import sys
    from pathlib import Path
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import tempdirs

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from benchmarking import measurement
from tlxlib import ROOT, interp_args, run_proc, tlx_env
from tlc import parse

from benchmarking import bench_budget

# A probe is one rustc invocation per revision: the shared deadline reserves
# this much for it, but lets it use whatever is left (up to PROBE_TIMEOUT_S).
PROBE_ESTIMATE_S = 60
PROBE_TIMEOUT_S = 600


def add_args(p):
    p.add_argument('--before', required=True, help='baseline git revision')
    p.add_argument('--before-bin', required=True)
    p.add_argument('--after-bin', default='target/release/tlx')
    p.add_argument('--sizes', default='4,8,12,16')
    p.add_argument('--repeat', type=int, default=3)
    p.add_argument('--timeout', type=int, default=60)
    p.add_argument('--out', default='bench/liveness.json')


def probe(directory, revision, name):
    """No copied implementation: compile the specified source verbatim.
    None when the benchmark deadline no longer fits the (estimated) build:
    the callers then record a skipped pass instead of starting rustc."""
    usable = bench_budget.current().admit(f'liveness:probe:{name}', PROBE_TIMEOUT_S, estimate=PROBE_ESTIMATE_S)
    if usable is None:
        return None
    d = directory / name
    d.mkdir()
    def source(path):
        return subprocess.check_output(['git', 'show', f'{revision}:{path}'], text=True, cwd=ROOT)
    tableau = source('crates/tlx-live/src/tableau.rs')
    (d / 'tableau.rs').write_text(tableau)
    optimized = 'for_negation_within' in tableau
    if optimized:
        (d / 'recurrence.rs').write_text(source('crates/tlx-live/src/recurrence.rs'))
    code = '''
extern crate tlx_base;
extern crate tlx_live;
pub use tlx_live::{Ltl, Bits};
mod search { pub type Within<'a> = dyn FnMut(usize, usize, usize) -> bool + 'a; }
mod tableau;
RECUR_MODULE
fn main() {
    let n: u32 = std::env::args().nth(1).unwrap().parse().unwrap();
    let prop = Ltl::Or((0..n).map(|i|
        Ltl::Eventually(Box::new(Ltl::Always(Box::new(Ltl::State(i)))))).collect());
    let t = std::time::Instant::now();
    let prop = std::hint::black_box(prop);
    CONSTRUCT
    let a = std::hint::black_box(a);
    let elapsed = t.elapsed().as_secs_f64();
    let edges: usize = a.states.iter().map(|s| s.succ.len()).sum();
    println!("{{\\"nodes\\":{},\\"edges\\":{},\\"conditions\\":{},\\"construction_s\\":{}}}",
        a.states.len(), edges, conditions, elapsed);
}
'''
    construct = ('let conditions = recurrence::for_negation(&prop).unwrap().len(); '
                 'let a = tableau::Automaton::universal();' if optimized else
                 'let a = tableau::Automaton::for_negation(&prop); let conditions = a.num_acc;')
    (d / 'main.rs').write_text(code.replace('RECUR_MODULE', 'mod recurrence;' if optimized else '').replace('CONSTRUCT', construct))
    deps = Path(ROOT) / 'target/release/deps'
    cmd = ['rustc', '--edition=2021', '--cfg', 'test', '-A', 'warnings', '-O', str(d / 'main.rs'), '-o', str(d / 'probe'),
           '-L', f'dependency={deps}']
    for lib in ['tlx_base', 'tlx_live']:
        rlib = max(deps.glob(f'lib{lib}-*.rlib'), key=lambda p: p.stat().st_mtime)
        cmd += ['--extern', f'{lib}={rlib}']
    subprocess.run(cmd, check=True, timeout=usable)
    return d / 'probe'


def measure(cmd, timeout, address_mib):
    # Bound the uninstrumented baseline too. Model checking needs room for
    # several 512 MiB virtual evaluation-thread stacks (not resident pages).
    # Use a wrapper rather than preexec_fn in a multithreaded Python process.
    if sys.platform == 'linux':
        cmd = [sys.executable, '-c', 'import os,resource,sys; limit=int(sys.argv[1])<<20; resource.setrlimit(resource.RLIMIT_AS,(limit,limit)); resource.setrlimit(resource.RLIMIT_CORE,(0,0)); os.execv(sys.argv[2],sys.argv[2:])', str(address_mib), *map(str, cmd)]
    p = run_proc(cmd, timeout, env=tlx_env(), rusage=True, raise_timeout=False, start_new_session=True)
    # (This child's own usage, exact rather than rounded: the scaling rows keep
    # the raw numbers the report compares.)
    return p, {'wall_s': p.wall, 'cpu_s': measurement.cpu_seconds(p.rusage, decimals=None),
               'rss_mib': measurement.rss_mib(p.rusage, decimals=None),
               'exit': p.returncode, 'output': p.stdout}


def run(a):
    budget = bench_budget.current()
    if budget.admit('liveness', 1) is None:
        return
    after = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    result = {'before': a.before, 'after': after, 'machine': os.environ.get('TLX_BENCH_MACHINE', ''),
              'timeout_s': a.timeout, 'address_limit_mib': {'automaton': 2048, 'model': 8192} if sys.platform == 'linux' else None, 'model_maxmem_mib': 2048, 'runs': []}
    if budget.until is not None:
        result['budget'] = {'skipped': budget.skipped}
    output = Path(a.out)
    output.parent.mkdir(parents=True, exist_ok=True)

    def checkpoint(row):
        # (Every run and every skip: a deadline that cuts the tail of the
        # scaling matrix keeps what was measured before it.)
        result['runs'].append(row)
        output.write_text(json.dumps(result, indent=2))
        print(json.dumps({k: v for k, v in row.items() if k != 'output'}), flush=True)

    with tempdirs.TemporaryDirectory(prefix='tlx-live-bench-') as tmp:
        d = Path(tmp)
        probes = {name: probe(d, rev, name) for name, rev in [('before', a.before), ('after', after)]}
        bins = {'before': Path(a.before_bin).resolve(), 'after': Path(a.after_bin).resolve()}
        for n in map(int, a.sizes.split(',')):
            spec = d / f'Clock{n}.tla'
            spec.write_text(f'''---- MODULE Clock{n} ----
EXTENDS Naturals
VARIABLE hr
Init == hr = 1
Next == hr' = IF hr = {n} THEN 1 ELSE hr + 1
Spec == Init /\\ [][Next]_hr /\\ WF_hr(Next)
ClockSettles == \\E k \\in 1..{n}: <>[](hr = k)
====
''')
            cfg = spec.with_suffix('.cfg')
            cfg.write_text('SPECIFICATION Spec\nPROPERTY ClockSettles\n')
            for rep in range(a.repeat):
                for name in (['before', 'after'] if rep % 2 == 0 else ['after', 'before']):
                    for phase in ['automaton', 'model']:
                        key = f'liveness:{phase}:{n}'
                        seconds = budget.admit(key, a.timeout)
                        if phase == 'automaton' and probes[name] is None:
                            # (The probe build did not fit the deadline; the
                            # uninstrumented model pass does not need it.)
                            checkpoint({'size': n, 'repeat': rep, 'binary': name, 'phase': phase,
                                        'result': 'skipped', 'reason': 'deadline', 'complete': False})
                            continue
                        if seconds is None:
                            checkpoint({'size': n, 'repeat': rep, 'binary': name, 'phase': phase,
                                        'result': 'skipped', 'reason': 'deadline', 'complete': False})
                            continue
                        cmd = [str(probes[name]), str(n)] if phase == 'automaton' else [str(bins[name]), 'check', str(spec), '-config', str(cfg), *interp_args(str(bins[name])), '-workers', '1', '-maxmem', '2048']
                        p, row = measure(cmd, seconds, 2048 if phase == 'automaton' else 8192)
                        budget.observe(key, row['wall_s'])
                        row.update(size=n, repeat=rep, binary=name, phase=phase)
                        if phase == 'automaton' and p.returncode == 0:
                            row.update(json.loads(p.stdout))
                            row['complete'] = True
                        elif phase == 'model':
                            row['result'] = parse(p.stdout, p.wall, p.returncode)
                            row['complete'] = row['result'].get('result') == 'liveness failure' and row['result'].get('distinct') == n
                        else:
                            row['complete'] = False
                        checkpoint(row)
    from benchmarking.bench_records import ingest
    ingest(output)
    if any(not r['complete'] for r in result['runs'] if r['binary'] == 'after'):
        note = f' ({len(budget.skipped)} pass(es) skipped at the deadline)' if budget.skipped else ''
        raise SystemExit('optimized runs did not all complete with the expected violation' + note)


if __name__ == '__main__':
    import argparse
    p = argparse.ArgumentParser(description=__doc__)
    add_args(p)
    run(p.parse_args())
