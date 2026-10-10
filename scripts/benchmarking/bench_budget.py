"""One budget across benchmark warmups, repetitions, binaries and model kinds."""
import functools
import inspect
import math
import time


class Budget:
    def __init__(self, minutes=None, deadline_at=None):
        if minutes is not None and (not math.isfinite(minutes) or minutes <= 0):
            raise ValueError('--deadline-min must be finite and positive')
        if deadline_at is not None and not math.isfinite(deadline_at):
            raise ValueError('--deadline-at must be finite')
        limits = ([minutes * 60] if minutes is not None else []) + ([deadline_at - time.time()] if deadline_at is not None else [])
        self.until = time.monotonic() + min(limits) if limits else None
        self.estimates, self.skipped, self.failures = {}, [], []

    def remaining(self):
        return max(0, self.until - time.monotonic() - 2) if self.until is not None else math.inf

    def admit(self, model, timeout, estimate=None):
        """Seconds the sample may run, or None to skip it.

        Only a *known* cost reserves budget: a measurement of this key here, else
        the caller's `estimate`, and both are capped by the enforced `timeout`
        (historical TLC costs can exceed a compiled sample's hard timeout). A key
        whose cost is not known yet reserves only what it can still consume here
        (`min(timeout, remaining)`) and is admitted while any budget is left, so a
        `--deadline-min` shorter than one `--timeout` measures the keys it can
        instead of skipping every one of them (a hard timeout is an upper bound,
        not an expected cost).
        """
        remaining = self.remaining()
        known = self.estimates.get(model, estimate)
        if known is not None and max(1, min(known, timeout)) > remaining:
            return self.skip(model, min(known, timeout), remaining)
        if remaining < 1:
            return self.skip(model, min(timeout, remaining), remaining)
        return min(timeout, remaining)

    def skip(self, model, expected, remaining):
        row = {'model': model, 'reason': 'deadline', 'estimate_s': round(expected, 3),
               'remaining_s': round(remaining, 3)}
        self.skipped.append(row)
        print(f'SKIP {model}: deadline; estimated {expected:.1f}s, remaining {remaining:.1f}s', flush=True)
        return None

    def observe(self, model, wall):
        self.estimates[model] = max(1, wall * 1.5 + 1)


_active = Budget()


def configure(args):
    global _active
    minutes = getattr(args, 'deadline_min', None)
    # ttfr already uses zero to mean unlimited.
    _active = Budget(minutes or None, getattr(args, 'deadline_at', None))
    return _active


def current():
    return _active


def fail(model, error):
    _active.failures.append({'model': model, 'error': error})
    print(f'FAIL {model}: {error}', flush=True)


def record(history):
    if _active.until is not None:
        history['budget'] = {'skipped': list(_active.skipped)}
    if _active.failures:
        history['compile_failures'] = list(_active.failures)


def model_pass(fn):
    """Guard the shared measurement entry points, including non-bench.py callers."""
    signature = inspect.signature(fn)
    @functools.wraps(fn)
    def run(*args, **kwargs):
        bound = signature.bind(*args, **kwargs)
        m = bound.arguments['m']
        mode = 'tlc' if 'tlc' in fn.__name__ else ('interp' if '-interp' in bound.arguments.get('args', []) else 'compiled')
        key = f"{m.get('id', m.get('tla', '?'))}:{mode}"
        estimate = m.get('estimate_s') or (m.get('tlc') or {}).get('wall_s')
        timeout = _active.admit(key, bound.arguments['timeout'], estimate=estimate)
        if timeout is None:
            return {'result': 'skipped', 'reason': 'deadline', 'wall_s': 0, 'cpu_s': 0, 'maxrss_mib': 0}
        bound.arguments['timeout'] = timeout
        result = fn(*bound.args, **bound.kwargs)
        if mode != 'tlc':
            result['compatibility'] = '-tlc' in bound.arguments.get('args', [])
        _active.observe(key, result.get('wall_s', timeout))
        if result.get('result') == 'compile failure':
            fail(key, result.get('error', 'whole-model compiler fallback'))
        return result
    return run
