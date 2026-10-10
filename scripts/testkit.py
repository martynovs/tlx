"""Shared helpers for the harness's Python test suite (``scripts/test_*.py``).

They make two classes of silent failure impossible to miss (2026-10-08, a
step-back review of the harness's test infrastructure):

* :class:`FakeClock` and :func:`with_fake_time` replace real time in a bounded
  retry loop with an injected, advancing clock, so a test drives minutes of
  backoff in microseconds instead of sleeping for real (one test once backed
  off for about ten minutes and had to be killed).
* :class:`NoPatchLeak` fails a test that leaves a ``unittest.mock`` patcher
  started. A bare ``patch(...).start()`` without a matching ``.stop()`` or
  ``addCleanup`` leaks process-wide and silently changes every later test (it
  broke ``test_merge_gate``'s progress tests once). ``GuardedTestCase`` is that
  mixin on top of ``unittest.TestCase``; harness tests should use it.
"""
import inspect
import unittest
from unittest import mock


# ``_active_patches`` is a module-level list every patcher appends to on
# ``start()`` and removes from on ``stop()``; its length is the leak signal.
_ACTIVE_PATCHES = mock._patch._active_patches


class FakeClock:
    """A deterministic clock for tests.

    ``clock()`` reads the current monotonic time; ``clock.sleep(secs)`` is a
    drop-in ``sleep`` that advances the clock instead of waiting, and records
    every slice in ``clock.sleeps`` so a test can assert the backoff schedule.
    ``clock.time`` is the wall-clock alias some call sites use.
    """

    def __init__(self, start=0.0):
        self.now = float(start)
        self.sleeps = []

    def __call__(self):
        return self.now

    def advance(self, seconds):
        self.now += float(seconds)
        return self.now

    def sleep(self, seconds):
        self.sleeps.append(float(seconds))
        self.now += float(seconds)
        return self.now

    # The wall clock many drivers read as ``time.time``; tests treat it as the
    # same monotonic instant, which is enough for a bounded loop's arithmetic.
    time = None  # set below, after the class body can reference the method


FakeClock.time = FakeClock.__call__


def with_fake_time(fn, *args, clock=None, **kwargs):
    """Call ``fn`` with an injected clock wherever it accepts one.

    ``fn``'s ``sleep`` parameter (if any) becomes ``clock.sleep`` and its
    ``monotonic``/``now`` parameter becomes ``clock`` itself, so a loop that
    backs off runs to completion without real waiting. The clock is returned
    alongside the result so a test can assert the schedule:
    ``result, clock = with_fake_time(loop, ...)``.
    """
    clock = FakeClock() if clock is None else clock
    parameters = inspect.signature(fn).parameters
    for name, value in (("sleep", clock.sleep), ("monotonic", clock), ("now", clock)):
        if name in parameters and name not in kwargs:
            kwargs[name] = value
    return fn(*args, **kwargs), clock


def patch_depth():
    """How many ``unittest.mock`` patchers are started right now."""
    return len(_ACTIVE_PATCHES)


def _patch_name(patcher):
    target = getattr(patcher, "attribute", None)
    return str(target) if target else repr(patcher)


class NoPatchLeak:
    """Mixin: fail a test that leaves a ``unittest.mock`` patcher started.

    Compose it *before* ``unittest.TestCase`` (``class T(NoPatchLeak,
    unittest.TestCase)``) or use :class:`GuardedTestCase`. The check runs as an
    ``addCleanup`` registered last, so it runs after ``tearDown`` and any
    cleanup that stops a patcher; it reports the leak as a failure of the test
    that caused it, naming the leaked target, and stops the leaked patchers so
    they cannot poison later tests too. A patch started before the test (a
    module-level ``setUpModule`` patch) is the baseline and never flagged.
    """

    def run(self, result=None):
        self.addCleanup(self._assert_no_patch_leak, patch_depth())
        return super().run(result)

    def _assert_no_patch_leak(self, before):
        leaked = _ACTIVE_PATCHES[before:]
        if not leaked:
            return
        del _ACTIVE_PATCHES[before:]
        for patcher in leaked:
            try:
                patcher.stop()
            except Exception:  # noqa: BLE001 - the leak is the failure, not the stop
                pass
        raise AssertionError(
            f"test left {len(leaked)} mock patcher(s) started; every .start() needs a "
            "matching .stop() or addCleanup(patcher.stop): " + ", ".join(_patch_name(p) for p in leaked))


class GuardedTestCase(NoPatchLeak, unittest.TestCase):
    """The shared base class for the harness's Python tests."""
