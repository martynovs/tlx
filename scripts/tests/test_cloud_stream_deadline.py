"""The transport itself must preserve the final result-fetch reserve."""
import contextlib
import io
import unittest
from unittest import mock

from cloud import cloud_common as common
import testkit


class StreamDeadline(unittest.TestCase):
    def test_ssh_forwards_the_callers_monotonic_deadline(self):
        machine = common.Machine.__new__(common.Machine)
        machine.ssh_cmd = mock.Mock(return_value=['ssh', 'fixture'])
        machine.run = mock.Mock(return_value=mock.Mock(returncode=0))
        machine.ssh('true', deadline=123)
        self.assertEqual(machine.run.call_args.kwargs['ssh_deadline'], 123)

    def test_result_probe_cannot_run_into_the_fetch_reserve(self):
        clock = testkit.FakeClock()
        machine = mock.Mock(dead_at=500.0, env={}, access=None, id='fixture-instance')
        machine.ssh_cmd.return_value = ['ssh', 'fixture']
        machine.ssh.side_effect = [mock.Mock(), mock.Mock(returncode=0, stdout='0')]
        with mock.patch.object(common.subprocess, 'Popen') as child, \
             mock.patch.object(common, 'log'), mock.patch.object(common.time, 'monotonic', return_value=1000):
            child.return_value.stdout = iter([])
            common.run_remote_verify(machine, ['true'], now=clock, sleep=clock.sleep)
        self.assertEqual(machine.ssh.call_args.kwargs.get('deadline'), 1200)
        self.assertEqual(machine.ssh.call_args_list[0].kwargs.get('deadline'), 1200)

    def test_stream_timer_is_bounded_and_cancelled_before_result_probe(self):
        clock = testkit.FakeClock()
        machine = mock.Mock(dead_at=500.0, env={}, access=None, id='fixture-instance')
        machine.ssh_cmd.return_value = ['ssh', 'fixture']
        machine.ssh.side_effect = [mock.Mock(), mock.Mock(returncode=0, stdout='0')]
        with mock.patch.object(common.subprocess, 'Popen') as child, \
             mock.patch.object(common.threading, 'Timer') as timer, mock.patch.object(common, 'log'):
            child.return_value.stdout = iter([])
            common.run_remote_verify(machine, ['true'], now=clock, sleep=clock.sleep)
        timer.assert_called_once()
        self.assertEqual(timer.call_args.args[0], 200)
        timer.return_value.start.assert_called_once()
        timer.return_value.cancel.assert_called_once()


if __name__ == '__main__':
    unittest.main()
