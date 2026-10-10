"""End-to-end provider request deadlines used by SSH recovery."""
import concurrent.futures
import io
import contextlib
import subprocess
import sys
from pathlib import Path
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from cloud import cloud_common
from cloud import transport, allowlist
import testkit


class RequestDeadline(unittest.TestCase):
    def test_cli_timeout_is_capped_by_the_repair_deadline(self):
        from cloud import deadline
        clock = testkit.FakeClock()
        with deadline.bound(20, now=clock), mock.patch.object(transport.subprocess, 'run') as run:
            run.return_value = subprocess.CompletedProcess([], 0, '{}', '')
            transport.capture_json(['tccli', 'vpc', 'Describe'], env={}, missing='missing', error=RuntimeError)
        self.assertEqual(run.call_args.kwargs['timeout'], 20)

    def test_expired_deadline_starts_no_provider_cli(self):
        from cloud import deadline
        with deadline.bound(0, now=lambda: 1), mock.patch.object(transport.subprocess, 'run') as run:
            with self.assertRaises(TimeoutError):
                transport.capture_json(['tccli', 'vpc', 'Describe'], env={}, missing='missing', error=RuntimeError)
        run.assert_not_called()

    def test_deadline_is_thread_local_and_nested_bounds_only_shrink(self):
        from cloud import deadline
        with deadline.bound(20, now=lambda: 0):
            self.assertEqual(deadline.remaining(120), 20)
            with deadline.bound(50, now=lambda: 0):
                self.assertEqual(deadline.remaining(120), 20)
            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                self.assertEqual(pool.submit(deadline.remaining, 120).result(timeout=5), 120)
        self.assertEqual(deadline.remaining(120), 120)

    def test_sdk_client_recomputes_budget_before_every_call(self):
        from cloud import deadline
        clock = testkit.FakeClock()
        budgets = []
        client = mock.Mock()
        proxy = deadline.Client(lambda budget: (budgets.append(budget), client)[1])
        with deadline.bound(20, now=clock):
            proxy.describe_instances(InstanceIds=['instance-test'])
            clock.advance(15)
            proxy.describe_instances(InstanceIds=['instance-test'])
            clock.advance(5)
            with self.assertRaises(TimeoutError):
                proxy.describe_instances(InstanceIds=['instance-test'])
        self.assertEqual(budgets, [20, 5])
        self.assertEqual(client.describe_instances.call_count, 2)

    def test_repair_calls_are_in_the_same_deadline_context(self):
        from cloud import deadline
        clock = testkit.FakeClock()
        provider = mock.Mock()
        seen = []
        def state(m):
            seen.append(deadline.remaining(120))
            clock.advance(160)
            return 'running'
        provider.instance_state.side_effect = state
        m = cloud_common.Machine(provider, cloud_common.Offer(1, 'r', 'z', 't', 16))
        m.id, m.dead_at = 'instance-test', 400
        access = allowlist.SshAccess(provider, m, ['203.0.113.1'], now=clock, sleep=clock.sleep)
        with mock.patch.object(cloud_common.time, 'time', return_value=0), \
             mock.patch.object(allowlist, 'warning'), mock.patch.object(allowlist, 'log'):
            result = access.run(mock.Mock(return_value=subprocess.CompletedProcess([], 255, '', 'Operation timed out')),
                                ['ssh', 'host', 'install packages'])
        self.assertEqual(seen, [120])
        provider.public_ips.assert_not_called()
        provider.allow_ssh.assert_not_called()
        self.assertIn('attempts: 1', result.ssh_diagnosis)


if __name__ == '__main__':
    unittest.main()
