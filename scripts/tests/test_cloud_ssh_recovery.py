#!/usr/bin/env python3
"""Shared-rule interleavings and post-boot transport recovery, without cloud access."""
import contextlib
import copy
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import threading
import time
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from cloud import cloud_common
from cloud import cloud_tencent
from cloud import cloud_aliyun
from cloud import cloud_aws
import cloud_verify
import testkit
from cloud import allowlist, sgpolicy, remote

OLD, NEW = '203.0.113.10', '203.0.113.20'


class Provider:
    ssh_user, ssh_ports = 'ubuntu', [22]

    def __init__(self, state='running'):
        self.status, self.writes, self.reads = state, [], 0

    def instance_state(self, m):
        self.reads += 1
        return self.status

    def public_ips(self, proxy):
        return [OLD]

    def allow_ssh(self, m, ips):
        self.writes.append(ips)
        return 'ssh rules restored'

    def allow_hint(self, m, ips):
        return 'add the operator address'


class Recovery(unittest.TestCase):
    def machine(self, provider=None, **kw):
        provider = provider or Provider()
        m = cloud_common.Machine(provider, cloud_common.Offer(1, 'region', 'zone', 'type', 16))
        m.id, m.ip = 'instance-test', '192.0.2.1'
        m.access = allowlist.SshAccess(provider, m, [OLD], sleep=lambda _: None, **kw)
        return m

    def test_default_ssh_captures_transport_failure_and_replays_packages(self):
        # A real child follows subprocess's capture semantics. A mocked result
        # with stderr filled even for capture=False hid this failure before.
        with tempfile.TemporaryDirectory() as directory:
            script = Path(directory) / 'ssh'
            counter = Path(directory) / 'count'
            script.write_text('#!' + sys.executable + '\n'
                              'from pathlib import Path\nimport sys\n'
                              f'p = Path({str(counter)!r})\n'
                              'n = int(p.read_text()) if p.exists() else 0\np.write_text(str(n+1))\n'
                              'if n < 2:\n print("Read from remote host: Operation timed out", file=sys.stderr)\n sys.exit(255)\n'
                              'print("packages ready")\n')
            script.chmod(0o700)
            m = self.machine()
            m.ssh_cmd = lambda: [str(script)]
            with contextlib.redirect_stdout(io.StringIO()) as out:
                result = m.ssh('install packages')
            self.assertEqual(result.returncode, 0)
            self.assertEqual(counter.read_text(), '3')
            self.assertIn('install packages', out.getvalue())
            self.assertIn('re-running', out.getvalue())

    def test_one_step_survives_more_than_one_transport_failure(self):
        m = self.machine()
        answers = iter([subprocess.CompletedProcess([], c, '', 'Operation timed out') for c in (255, 255, 0)])
        with mock.patch.object(allowlist, 'log'), mock.patch.object(allowlist, 'warning'):
            p = m.access.run(mock.Mock(side_effect=answers), ['ssh', 'host', 'install packages'])
        self.assertEqual(p.returncode, 0)
        self.assertEqual(len(m.provider.writes), 2)

    def test_stop_names_provider_state_and_step_attempts(self):
        m = self.machine()
        with mock.patch.object(cloud_common.subprocess, 'run', return_value=subprocess.CompletedProcess(
                [], 255, '', 'Operation timed out')), mock.patch.object(allowlist, 'warning'), mock.patch.object(allowlist, 'log'):
            with self.assertRaises(cloud_common.InfraError) as raised:
                m.ssh('install packages', capture=True)
        message = str(raised.exception)
        self.assertIn('provider state: running', message)
        self.assertIn('attempts: 4', message)
        self.assertIn('install packages', message)
        self.assertEqual(m.provider.reads, 3)

    def test_a_later_outage_gets_a_fresh_rate_budget(self):
        clock = testkit.FakeClock()
        m = self.machine(now=clock, recharge=900)
        down = subprocess.CompletedProcess([], 255, '', 'Operation timed out')
        with mock.patch.object(allowlist, 'warning'), mock.patch.object(allowlist, 'log'):
            m.access.run(mock.Mock(return_value=down), ['ssh', 'host', 'probe'])
            self.assertEqual(len(m.provider.writes), 3)
            clock.advance(900)
            self.assertTrue(m.access.repair('Operation timed out'))
        self.assertEqual(len(m.provider.writes), 4)

    def test_disabled_recharge_is_a_hard_bound(self):
        clock = testkit.FakeClock()
        m = self.machine(now=clock, recharge=None, rounds=1)
        with mock.patch.object(allowlist, 'warning'), mock.patch.object(allowlist, 'log'):
            self.assertTrue(m.access.repair('Operation timed out'))
            clock.advance(10000)
            self.assertFalse(m.access.repair('Operation timed out'))
        self.assertEqual(len(m.provider.writes), 1)

    def test_nontransport_remote_failure_is_never_replayed(self):
        m = self.machine()
        failed = subprocess.CompletedProcess([], 2, '', 'package missing')
        attempt = mock.Mock(return_value=failed)
        self.assertIs(m.access.run(attempt, ['ssh', 'host', 'install packages']), failed)
        self.assertEqual(attempt.call_count, 1)
        self.assertEqual(m.provider.reads, 0)

    def test_parallel_repairs_share_one_rate_budget(self):
        m = self.machine()
        barrier = threading.Barrier(8)
        results = []
        def repair():
            barrier.wait(timeout=5)
            results.append(m.access.repair('Operation timed out'))
        threads = [threading.Thread(target=repair) for _ in range(8)]
        with mock.patch.object(allowlist, 'log'), mock.patch.object(allowlist, 'warning'):
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join(timeout=5)
                self.assertFalse(thread.is_alive())
        self.assertEqual(sum(results), 3)
        self.assertEqual(m.provider.reads, 3)
        self.assertEqual(len(m.provider.writes), 3)

    def test_stopped_instance_is_not_replayed(self):
        m = self.machine(Provider('stopped'))
        down = subprocess.CompletedProcess([], 255, '', 'Operation timed out')
        attempt = mock.Mock(return_value=down)
        with mock.patch.object(allowlist, 'warning'):
            result = m.access.run(attempt, ['ssh', 'host', 'install packages'])
        self.assertEqual(attempt.call_count, 1)
        self.assertIn('provider state: stopped', result.ssh_diagnosis)
        self.assertIn('repairs: 0', result.ssh_diagnosis)

    def test_slow_repair_does_not_start_an_attempt_past_the_deadline(self):
        clock = testkit.FakeClock()
        m = self.machine(now=clock)
        m.dead_at = 400
        m.access._sleep = lambda seconds: clock.advance(300)
        down = subprocess.CompletedProcess([], 255, '', 'Operation timed out')
        attempt = mock.Mock(return_value=down)
        with mock.patch.object(cloud_common.time, 'time', return_value=0), \
             mock.patch.object(allowlist, 'warning'), mock.patch.object(allowlist, 'log'):
            result = m.access.run(attempt, ['ssh', 'host', 'install packages'])
        self.assertEqual(attempt.call_count, 1)
        self.assertIn('deadline', result.ssh_diagnosis)

    def test_server_alive_timeout_is_a_postboot_transport_failure(self):
        m = self.machine()
        answers = iter([subprocess.CompletedProcess([], 255, '', 'Timeout, server 192.0.2.1 not responding.'),
                        subprocess.CompletedProcess([], 0, '', '')])
        with mock.patch.object(allowlist, 'warning'), mock.patch.object(allowlist, 'log'):
            result = m.access.run(mock.Mock(side_effect=answers), ['ssh', 'host', 'install packages'])
        self.assertEqual(result.returncode, 0)
        self.assertEqual(m.provider.reads, 1)

    def test_twenty_seconds_left_never_starts_a_slow_provider_repair(self):
        clock = testkit.FakeClock()
        m = self.machine(now=clock)
        m.dead_at = 140
        def slow_state(machine):
            clock.advance(120)
            return 'running'
        m.provider.instance_state = mock.Mock(side_effect=slow_state)
        with mock.patch.object(cloud_common.time, 'time', return_value=0), \
             mock.patch.object(allowlist, 'warning'), mock.patch.object(allowlist, 'log'):
            result = m.access.run(mock.Mock(return_value=subprocess.CompletedProcess([], 255, '', 'Operation timed out')),
                                  ['ssh', 'host', 'install packages'])
        m.provider.instance_state.assert_not_called()
        self.assertEqual(clock(), 0)
        self.assertIn('cleanup reserve', result.ssh_diagnosis)

    def test_cleanup_reserve_stops_replays(self):
        m = self.machine(now=lambda: 0)
        m.dead_at = 120
        down = subprocess.CompletedProcess([], 255, '', 'Operation timed out')
        attempt = mock.Mock(return_value=down)
        with mock.patch.object(cloud_common.time, 'time', return_value=0):
            result = m.access.run(attempt, ['ssh', 'host', 'install packages'])
        self.assertEqual(attempt.call_count, 1)
        self.assertEqual(m.provider.reads, 0)
        self.assertIn('budget exhausted', result.ssh_diagnosis)



class SharedRules:
    """One provider-owned rule set, mutated by every launch and repair.

    Reads are snapshots; Tencent indexes address positions in the *current*
    list, so a nested launch after the read faithfully shifts those positions.
    A whole-set replacement is refused rather than silently accepted by a mock.
    """
    def __init__(self, kind):
        self.kind, self.rows, self.after_read, self.serial = kind, [], None, 0

    def read(self):
        rows = copy.deepcopy(self.rows)
        if self.after_read:
            callback, self.after_read = self.after_read, None
            callback()
        return rows

    def add(self, port, cidr, owner):
        if any(r['port'] == port and r['cidr'] == cidr for r in self.rows):
            return
        self.serial += 1
        self.rows.insert(0, dict(port=port, cidr=cidr, owner=owner, id=str(self.serial)))

    def tencent(self, product, api, region=None, **kw):
        if api == 'DescribeSecurityGroupPolicies':
            version = str(self.serial)
            rows = self.read()
            return {'SecurityGroupPolicySet': {'Ingress': [dict(PolicyIndex=i, Protocol='TCP', Port=r['port'],
                    CidrBlock=r['cidr'], Action='ACCEPT', PolicyDescription=r['owner']) for i, r in enumerate(rows)],
                    'Version': version, 'Egress': [dict(Action='ACCEPT', CidrBlock='0.0.0.0/0')]}}
        if api == 'CreateSecurityGroupPolicies':
            for r in kw['SecurityGroupPolicySet'].get('Ingress', []):
                self.add(r['Port'], r['CidrBlock'], r['PolicyDescription'])
            return {}
        if api == 'DeleteSecurityGroupPolicies':
            version = kw['SecurityGroupPolicySet'].get('Version')
            if version is not None and version != str(self.serial):
                raise cloud_common.InfraError('policy version changed')
            for i in sorted((r['PolicyIndex'] for r in kw['SecurityGroupPolicySet']['Ingress']), reverse=True):
                del self.rows[i]
            return {}
        raise AssertionError('unexpected or destructive API: ' + api)

    def aliyun(self, product, api, region=None, *args):
        args = dict(zip(args[::2], args[1::2]))
        if api == 'DescribeSecurityGroupAttribute':
            return {'Permissions': {'Permission': [dict(IpProtocol='tcp', PortRange=r['port'], SourceCidrIp=r['cidr'],
                    Policy='ACCEPT', Description=r['owner'], SecurityGroupRuleId=r['id']) for r in self.read()]}}
        if api == 'AuthorizeSecurityGroup':
            self.add(args['--PortRange'], args['--SourceCidrIp'], args['--Description'])
            return {}
        if api == 'RevokeSecurityGroup':
            ids = {v for k, v in args.items() if k.startswith('--SecurityGroupRuleId.')}
            self.rows[:] = [r for r in self.rows if r['id'] not in ids]
            return {}
        raise AssertionError('unexpected or destructive API: ' + api)

    def describe_security_groups(self, **kw):
        return {'SecurityGroups': [{'GroupId': 'sg-test', 'IpPermissions': [dict(IpProtocol='tcp', FromPort=22,
                ToPort=22, IpRanges=[dict(CidrIp=r['cidr'], Description=r['owner'])]) for r in self.read()]}]}

    def describe_security_group_rules(self, **kw):
        return {'SecurityGroupRules': [dict(SecurityGroupRuleId=r['id'], IsEgress=False, Description=r['owner'])
                                       for r in self.rows]}

    def authorize_security_group_ingress(self, **kw):
        for p in kw['IpPermissions']:
            for r in p['IpRanges']:
                self.add(str(p['FromPort']), r['CidrIp'], r['Description'])

    def revoke_security_group_ingress(self, **kw):
        if 'SecurityGroupRuleIds' in kw:
            self.rows[:] = [r for r in self.rows if r['id'] not in kw['SecurityGroupRuleIds']]
            return
        cidrs = {r['CidrIp'] for p in kw['IpPermissions'] for r in p['IpRanges']}
        self.rows[:] = [r for r in self.rows if r['cidr'] not in cidrs]

    @contextlib.contextmanager
    def provider(self, owner):
        cls = {'tencent': cloud_tencent.TencentProvider, 'aliyun': cloud_aliyun.AliyunProvider,
               'aws': cloud_aws.AwsProvider}[self.kind]
        p = cls.__new__(cls)
        p.owner = owner
        p.find_security_group = lambda *args: 'sg-test'
        p.find_vpc = lambda *args: 'vpc-test'
        p.default_vpc = lambda *args: 'vpc-test'
        with contextlib.ExitStack() as stack:
            if self.kind == 'tencent':
                stack.enter_context(mock.patch.object(cloud_tencent, 'tccli', self.tencent))
            elif self.kind == 'aliyun':
                stack.enter_context(mock.patch.object(cloud_aliyun, 'aliyun', self.aliyun))
            else:
                p.ec2 = lambda *args: self
            yield p

    def launch(self, owner, ip):
        with self.provider(owner) as p:
            if self.kind == 'tencent':
                p.ensure_security_group('region', [ip])
            elif self.kind == 'aliyun':
                p.ensure_security_group('region', 'vpc-test', [ip])
            else:
                p.ensure_security_group(self, 'vpc-test', [ip])


class SharedGroup(unittest.TestCase):
    def test_borrowed_rules_survive_owner_moving_and_concurrent_launch(self):
        for kind in ('tencent', 'aliyun', 'aws'):
            with self.subTest(provider=kind), contextlib.redirect_stdout(io.StringIO()):
                group = SharedRules(kind)
                group.launch('owner-a', OLD)
                group.launch('owner-b', OLD)  # B borrows A's address, as in the incident.
                borrowed = copy.deepcopy(group.rows)
                # After A reads, B adds another address. Tencent's positional
                # indexes shift; all providers must preserve B's borrowed access.
                group.after_read = lambda: group.launch('owner-b', '203.0.113.30')
                group.launch('owner-a', NEW)
                self.assertTrue(all(r in group.rows for r in borrowed), group.rows)
                self.assertTrue(any(r['cidr'] == '203.0.113.30/32' for r in group.rows))
                before = copy.deepcopy(group.rows)
                with group.provider('owner-b') as p:
                    m = mock.Mock(region='region', proxy=None, id='instance-test')
                    p.allow_ssh(m, [OLD])
                self.assertEqual(group.rows, before)


class Prune(unittest.TestCase):
    def test_refuses_any_tagged_instance_before_deleting_rules(self):
        provider = mock.Mock(all_regions=['region'], name='tencent')
        provider.leftovers.return_value = [('region', 'instance-test', 'RUNNING')]
        ledger = mock.Mock()
        ledger.locked = lambda: contextlib.nullcontext([])
        with mock.patch.object(cloud_verify, 'check_coordinator'):
            with self.assertRaises(cloud_common.CloudStop):
                cloud_verify.prune_security_groups(provider, ledger, ['region'])
        provider.prune_ssh_rules.assert_not_called()
        provider.leftovers.assert_called_once_with(['region'])

    def test_prunes_only_with_no_instances_under_launch_lock(self):
        provider = mock.Mock(all_regions=['region'], name='tencent')
        provider.leftovers.return_value = []
        events = []
        ledger = mock.Mock()
        @contextlib.contextmanager
        def locked():
            events.append('locked')
            yield []
            events.append('unlocked')
        ledger.locked = locked
        provider.prune_ssh_rules.side_effect = lambda regions: events.append('pruned')
        with mock.patch.object(cloud_verify, 'check_coordinator'):
            cloud_verify.prune_security_groups(provider, ledger, ['region'])
        self.assertEqual(events, ['locked', 'pruned', 'unlocked'])

    def test_shared_prune_preserves_untagged_and_manual_rules_on_every_provider(self):
        for kind in ('tencent', 'aliyun', 'aws'):
            with self.subTest(provider=kind), contextlib.redirect_stdout(io.StringIO()):
                group = SharedRules(kind)
                group.launch('owner-a', OLD)
                port = '22/22' if kind == 'aliyun' else '22'
                group.add(port, '192.0.2.1/32', '')
                group.add(port, '192.0.2.2/32', 'cloud_verify')
                group.add(port, '192.0.2.3/32', 'manual rule')
                manual = [r for r in group.rows if not r['owner'].startswith('cloud_verify:')]
                with group.provider('owner-a') as p:
                    p.prune_ssh_rules(['region'])
                self.assertEqual(group.rows, manual)

    def test_worker_prune_is_refused_before_provider_calls(self):
        # Explicit git responses keep this test valid in staged synthetic
        # checkouts too; no real account/config may be touched by this refusal.
        with tempfile.TemporaryDirectory() as directory:
            git = Path(directory) / 'git'
            git.write_text('#!' + sys.executable + '\nimport sys\n'
                           'print("/fake/shared.git" if "--git-common-dir" in sys.argv else "/fake/worker.git")\n')
            git.chmod(0o700)
            env = dict(os.environ, PATH=directory + os.pathsep + os.environ['PATH'])
            result = subprocess.run([sys.executable, str(Path(cloud_verify.__file__)), '--cleanup', '--prune-sg'],
                                    env=env, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 25)
        self.assertIn('coordinator-only', result.stdout)

    def test_recent_launch_hidden_by_listing_lag_blocks_prune(self):
        provider = mock.Mock(all_regions=['region'])
        provider.name = 'tencent'
        ledger = mock.Mock()
        ledger.locked = lambda: contextlib.nullcontext([dict(provider='tencent', status='launched', time=time.time())])
        with mock.patch.object(cloud_verify, 'check_coordinator'):
            with self.assertRaises(cloud_common.CloudStop):
                cloud_verify.prune_security_groups(provider, ledger, ['region'])
        provider.prune_ssh_rules.assert_not_called()

    def test_tencent_version_check_preserves_concurrent_manual_rule(self):
        group = SharedRules('tencent')
        with contextlib.redirect_stdout(io.StringIO()):
            group.launch('owner-a', OLD)
        group.after_read = lambda: group.add('22', '192.0.2.5/32', 'manual rule')
        with group.provider('owner-a') as p:
            with self.assertRaisesRegex(cloud_common.InfraError, 'version changed'):
                p.prune_ssh_rules(['region'])
        self.assertEqual(len(group.rows), 4)


    def test_launch_reports_rule_capacity_for_every_provider(self):
        for kind, limit in [('tencent', 100), ('aliyun', 200), ('aws', 60)]:
            with self.subTest(provider=kind), contextlib.redirect_stdout(io.StringIO()) as out:
                SharedRules(kind).launch('owner-a', OLD)
            self.assertIn('0 existing ingress rule(s)', out.getvalue())
            self.assertIn(f'limit {limit}', out.getvalue())



class DetachedJob(unittest.TestCase):
    def test_replaying_start_executes_the_job_once_even_after_it_finishes(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            (home / 'tlx').mkdir()
            (home / 'tlx-env.sh').write_text('true\n')
            counter = home / 'count'
            job = f"echo ran >> {counter}"
            script = cloud_common.remote_start_script(job, 'test-token')
            env = dict(os.environ, HOME=directory)
            for _ in range(2):
                subprocess.run(['sh', '-c', script], env=env, capture_output=True, timeout=10, check=True)
                deadline = time.monotonic() + 5
                while not (home / 'verify.rc').exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertTrue((home / 'verify.rc').exists())
            self.assertEqual(counter.read_text(), 'ran\n')

    def test_delayed_start_uses_absolute_deadline_and_runs_no_expired_job(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            (home / 'tlx').mkdir()
            (home / 'tlx-env.sh').write_text('true\n')
            counter = home / 'count'
            script = cloud_common.remote_start_script(f'echo ran >> {counter}', 'expired-test', deadline_at=time.time() - 1)
            subprocess.run(['sh', '-c', script], env=dict(os.environ, HOME=directory),
                           capture_output=True, timeout=10, check=True)
            deadline = time.monotonic() + 5
            while not (home / 'verify.rc').exists() and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertEqual((home / 'verify.rc').read_text(), '124')
            self.assertFalse(counter.exists())

    @staticmethod
    def stream_of(clock, secs):
        """A streaming stdout that carries the job for `secs` before it ends."""
        class Stream:
            def __init__(self):
                self.done = False

            def __iter__(self):
                return self

            def __next__(self):
                if self.done:
                    raise StopIteration
                self.done = True
                clock.advance(secs)
                return ""

        return Stream()

    def test_a_long_stream_may_end_many_times_without_failing_the_run(self):
        # ap-shanghai-5 drops the streaming connection of a quiet detached job
        # about every 10 minutes (keepalives do not survive the middlebox), so a
        # healthy 2.5h run reconnects twenty-plus times. The fail-fast cap must
        # not abort it: the 2026-10-09 MCetcdraft_3s_t2 lane died at reconnect
        # 23 with "output stream repeatedly ended while ssh was reachable".
        clock = testkit.FakeClock()
        m = mock.Mock(dead_at=500_000.0, env={})
        m.ssh_cmd.return_value = ['ssh', 'host']
        empty = mock.Mock(returncode=0, stdout='')
        m.ssh.side_effect = [mock.Mock()] + [empty] * 25 + [mock.Mock(returncode=0, stdout='0\n')]

        def stream_once(*args, **kwargs):
            # Every reconnect opens a fresh streaming ssh; a single shared
            # Stream would raise StopIteration without advancing the clock on
            # the second call and look like an instantly dying stream.
            opened = mock.Mock()
            opened.stdout = self.stream_of(clock, 600)
            return opened

        with mock.patch.object(cloud_common.subprocess, 'Popen', side_effect=stream_once), \
             mock.patch.object(cloud_common, 'log'), mock.patch.object(cloud_common, 'warning'):
            code, _ = cloud_common.run_remote_verify(m, ['true'], now=clock, sleep=clock.sleep)
        self.assertEqual(code, 0)
        self.assertEqual(len(clock.sleeps), 25)

    def test_immediate_stream_breaks_wait_until_the_fetch_reserve(self):
        # A short-lived transport is no stronger evidence of job failure than
        # a long-lived one. Only the time reserve ends recovery.
        clock = testkit.FakeClock()
        m = mock.Mock(dead_at=500.0, env={}, access=None, id='fixture-instance')
        m.ssh_cmd.return_value = ['ssh', 'host']
        m.ssh.return_value = mock.Mock(returncode=0, stdout='')
        with mock.patch.object(cloud_common.subprocess, 'Popen') as child, \
             mock.patch.object(cloud_common, 'log'), mock.patch.object(cloud_common, 'warning'):
            child.return_value.stdout = iter([])
            with self.assertRaisesRegex(cloud_common.InfraError, 'final result fetch'):
                cloud_common.run_remote_verify(m, ['true'], now=clock, sleep=clock.sleep)
        self.assertEqual(clock(), 200)
        self.assertEqual(len(clock.sleeps), 40)

    def test_outage_reconnects_beyond_twenty_attempts_until_deadline(self):
        clock = testkit.FakeClock()
        m = mock.Mock(dead_at=5000.0, env={})
        m.ssh_cmd.return_value = ['ssh', 'host']
        down = mock.Mock(returncode=255, stdout='')
        m.ssh.side_effect = [mock.Mock()] + [down] * 25 + [mock.Mock(returncode=0, stdout='0\n')]
        with mock.patch.object(cloud_common.subprocess, 'Popen') as child, \
             mock.patch.object(cloud_common, 'log'), mock.patch.object(cloud_common, 'warning'):
            child.return_value.stdout = iter([])
            code, _ = cloud_common.run_remote_verify(m, ['true'], now=clock, sleep=clock.sleep)
        self.assertEqual(code, 0)
        self.assertEqual(len(clock.sleeps), 25)

    def test_outage_stops_with_cleanup_time_left(self):
        clock = testkit.FakeClock()
        m = mock.Mock(dead_at=400.0, env={}, access=None)
        m.ssh_cmd.return_value = ['ssh', 'host']
        m.ssh.return_value = mock.Mock(returncode=255, stdout='', ssh_diagnosis='provider state: running; repairs: 3')
        with mock.patch.object(cloud_common.subprocess, 'Popen') as child, \
             mock.patch.object(cloud_common, 'log'), mock.patch.object(cloud_common, 'warning'):
            child.return_value.stdout = iter([])
            with self.assertRaises(cloud_common.InfraError) as raised:
                cloud_common.run_remote_verify(m, ['true'], now=clock, sleep=clock.sleep)
        self.assertEqual(clock(), 100)
        self.assertIn('provider state: running', str(raised.exception))
        self.assertIn('before the dead-man', str(raised.exception))

    def test_final_transfer_survives_an_outage_longer_than_ten_minutes(self):
        clock = testkit.FakeClock()
        m = mock.Mock()
        def attempt(*args, **kwargs):
            clock.advance(200)
            return subprocess.CompletedProcess([], 0 if clock() > 700 else 255, b'', b'Operation timed out')
        m.run.side_effect = attempt
        with mock.patch.object(cloud_common, 'warning'):
            cloud_common.required_rsync(m, ['rsync', '-e', 'ssh', 'remote', 'local'], 'results',
                                        sleep=clock.sleep, monotonic=clock)
        self.assertEqual(m.run.call_count, 4)


    def test_expired_transfer_deadline_starts_no_io(self):
        m = mock.Mock()
        m.run.return_value = subprocess.CompletedProcess([], 0, b'', b'')
        with self.assertRaises(cloud_common.InfraError):
            cloud_common.required_rsync(m, ['rsync', '-e', 'ssh', 'src', 'dst'], 'results',
                                        deadline=0, monotonic=lambda: 1)
        m.run.assert_not_called()

    def test_final_transfer_passes_deadline_to_each_transport_attempt(self):
        m = mock.Mock()
        m.run.return_value = subprocess.CompletedProcess([], 0, b'', b'')
        cloud_common.required_rsync(m, ['rsync', '-e', 'ssh', 'src', 'dst'], 'results',
                                    deadline=50, monotonic=lambda: 1)
        self.assertEqual(m.run.call_args.kwargs['ssh_deadline'], 50)

    def test_every_fetcher_passes_the_full_transfer_deadline(self):
        m = mock.Mock(dead_at=10000)
        for factory, fn in [(remote._bench_fetcher, 'fetch_bench_results'),
                            (remote._oracle_fetcher, 'fetch_oracle_results'),
                            (lambda m, d: remote._results_fetcher(m, d)[0], 'fetch_results')]:
            with self.subTest(fetcher=fn), mock.patch.object(cloud_common.time, 'time', return_value=1000), \
                 mock.patch.object(cloud_common.time, 'monotonic', return_value=0), mock.patch.object(remote, fn) as fetch:
                factory(m, 'local')(False)
                self.assertEqual(fetch.call_args.kwargs['deadline'], 3600)



if __name__ == '__main__':
    unittest.main()
