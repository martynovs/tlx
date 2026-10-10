"""Detached results are collected on every exit path before instance release."""
import contextlib
import io
import unittest
from unittest import mock

from cloud import remote
from cloud.session import Session
from cloud import session as session_module
from cloud.cloud_common import Offer
from cloud.cloud_common import InfraError, Interrupted


class FinalFetch(unittest.TestCase):
    def test_failures_fetch_existing_results_before_session_cleanup(self):
        for error in (InfraError('transport deadline'), Interrupted('cancel'), KeyboardInterrupt()):
            with self.subTest(error=type(error).__name__):
                events = []
                session = Session(a=mock.Mock(keep_instance=False),
                                  plan=mock.Mock(currency='CNY', local_dir='/nonexistent-fixture-results'),
                                  cancel=mock.Mock(), hooks=mock.Mock())
                session.machine = mock.Mock(id='fixture-instance', terminated=False, ledger_time=None,
                                            offer=Offer(1, 'region', 'zone', 'fixture', 16))
                session.machine.wall.return_value = 1.0
                session.machine.cost.return_value = 0.01
                fetch = mock.Mock(side_effect=lambda quiet: events.append('fetch') or 123)
                def passes(s):
                    remote.run_pass(s, ['true'], 'verify', fetch)
                with mock.patch.object(remote, 'boot'), mock.patch.object(remote, 'prepare'), \
                     mock.patch.object(remote, 'passes', side_effect=passes), \
                     mock.patch.object(remote, 'run_remote_verify', side_effect=error), \
                     mock.patch.object(remote, 'periodic_fetch'), \
                     mock.patch.object(session_module.signal, 'signal'), \
                     mock.patch.object(session_module, 'terminate_until_gone', side_effect=lambda *a: events.append('terminate') or True) as terminate, \
                     contextlib.redirect_stdout(io.StringIO()):
                    session.run()
                self.assertEqual(events, ['fetch', 'terminate'])
                self.assertEqual(session.fetched_at, 123)
                self.assertTrue(session.machine.terminated)
                terminate.assert_called_once_with(session.plan.provider, session.machine)

    def test_fetch_failure_names_instance_and_loss(self):
        session = mock.Mock(machine=mock.Mock(id='fixture-instance'), phases={})
        with mock.patch.object(remote, 'run_remote_verify', return_value=(0, 'results')), \
             mock.patch.object(remote, 'periodic_fetch'), \
             self.assertRaisesRegex(InfraError, 'fixture-instance.*results may be lost'):
            remote.run_pass(session, ['true'], 'verify', mock.Mock(side_effect=OSError('network unavailable')))


if __name__ == '__main__':
    unittest.main()
