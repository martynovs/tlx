"""The shared test helpers enforce their own two rules (scripts/testkit.py)."""
import unittest
from unittest import mock

import testkit


class FakeClockTests(testkit.GuardedTestCase):
    def test_sleep_advances_the_clock_and_records_the_slices(self):
        clock = testkit.FakeClock()
        clock.advance(2)
        clock.sleep(5)
        self.assertEqual(clock(), 7)
        self.assertEqual(clock.time(), 7)
        self.assertEqual(clock.sleeps, [5])

    def test_with_fake_time_injects_the_clock_into_a_real_backoff_loop(self):
        from cloud import cloud_common

        calls = []

        def flaky():
            calls.append(1)
            if len(calls) < 4:
                raise ConnectionResetError("name resolution error")
            return "done"

        # retry_network backs off 5 + 10 + 20 s in real time; the fake clock
        # runs it to completion without waiting (the incident this rule fixes).
        result, clock = testkit.with_fake_time(
            cloud_common.retry_network, flaky, "unit", budget=600,
            retry=cloud_common.is_network_error, clock=testkit.FakeClock())
        self.assertEqual(result, "done")
        self.assertEqual(clock.sleeps, [5, 10, 20])



class NoPatchLeakTests(testkit.GuardedTestCase):
    """Fail-before/pass-after: the same leaking test passes unguarded and fails guarded."""

    def test_a_leaked_patcher_is_caught_and_cleaned_up(self):
        class Leaky(testkit.GuardedTestCase):
            def test_leaks(self):
                mock.patch("time.sleep").start()  # no stop, no addCleanup

        before = testkit.patch_depth()
        result = unittest.TestResult()
        Leaky("test_leaks").run(result)
        self.assertFalse(result.wasSuccessful())
        report = "".join(text for _, text in result.failures + result.errors)
        self.assertIn("patcher", report)
        self.assertIn("sleep", report)
        self.assertEqual(testkit.patch_depth(), before)  # the guard stopped it

    def test_the_same_leak_would_pass_without_the_guard(self):
        class Leaky(unittest.TestCase):
            def test_leaks(self):
                mock.patch("time.sleep").start()

        before = testkit.patch_depth()
        result = unittest.TestResult()
        Leaky("test_leaks").run(result)
        # The unguarded leak is exactly the silent failure: green test, dirty
        # process-wide state.
        self.assertTrue(result.wasSuccessful())
        self.assertEqual(testkit.patch_depth(), before + 1)
        for patcher in mock._patch._active_patches[before:]:
            patcher.stop()

    def test_a_stopped_patcher_passes(self):
        class Clean(testkit.GuardedTestCase):
            def test_ok(self):
                patcher = mock.patch("time.sleep")
                patcher.start()
                self.addCleanup(patcher.stop)

        result = unittest.TestResult()
        Clean("test_ok").run(result)
        self.assertTrue(result.wasSuccessful())

    def test_a_patch_started_before_the_test_is_not_flagged(self):
        patcher = mock.patch("time.sleep")
        patcher.start()
        try:
            class ModuleLevel(testkit.GuardedTestCase):
                def test_ok(self):
                    pass

            result = unittest.TestResult()
            ModuleLevel("test_ok").run(result)
            self.assertTrue(result.wasSuccessful())
        finally:
            patcher.stop()


class GuardedBaseTests(testkit.GuardedTestCase):
    def test_the_shared_base_composes_the_mixin_and_testcase(self):
        self.assertTrue(issubclass(testkit.GuardedTestCase, testkit.NoPatchLeak))
        self.assertTrue(issubclass(testkit.GuardedTestCase, unittest.TestCase))


if __name__ == "__main__":
    unittest.main()
