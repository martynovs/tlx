#!/usr/bin/env python3
"""Unit tests of scripts/cloud/sgpolicy.py, the shared security-group policy.

The group (`tlx-verify-ssh`) is shared by every owner on this machine, so the
decision of what a launch may change is made in one place and used by all three
providers (scripts/cloud_{tencent,aliyun,aws}.py). The provider-level cases -
the incident itself, on each provider's own API calls - are in
test_cloud_verify.py (SharedSecurityGroup); this module locks the policy, the
rule description that carries ownership, and the repair path's control.

usage: python3 scripts/tests/test_cloud_sgpolicy.py [-v]
"""
import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from cloud import cloud_common
import testkit
from cloud import allowlist as cloud_allowlist
from cloud import sgpolicy

OLD, NEW, THIRD = "1.202.18.10", "61.149.241.75", "150.158.149.193"


def rule(port="22", cidr=f"{OLD}/32", description="", protocol="TCP", action="ACCEPT", ref=0):
    return sgpolicy.Ingress(protocol=protocol, port=port, cidr=cidr, description=description,
                            action=action, ref=ref)


class Description(testkit.GuardedTestCase):
    """A rule's description is the only thing that says who wrote it, so it is
    part of the on-the-wire contract: a process that is already running reads
    it, and an owner tag has to survive the tightest provider limit."""

    def test_a_rule_names_the_owner_that_wrote_it(self):
        self.assertEqual(sgpolicy.describe("owner-a"), "cloud_verify:owner-a")
        self.assertTrue(sgpolicy.names_owner(sgpolicy.describe("owner-a")))
        self.assertTrue(sgpolicy.is_ours(sgpolicy.describe("owner-a"), "owner-a"))
        self.assertFalse(sgpolicy.is_ours(sgpolicy.describe("owner-a"), "owner-b"))

    def test_the_marker_alone_names_nobody(self):
        # What every rule written before owner tags existed carries: it is not
        # ours (any owner that ran that code could have written it).
        self.assertEqual(sgpolicy.describe(""), "cloud_verify")
        self.assertFalse(sgpolicy.names_owner("cloud_verify"))
        self.assertFalse(sgpolicy.is_ours("cloud_verify", "owner-a"))
        self.assertFalse(sgpolicy.is_ours("", "owner-a"))
        self.assertFalse(sgpolicy.is_ours("something else entirely", "owner-a"))

    def test_a_run_without_an_owner_owns_nothing(self):
        self.assertFalse(sgpolicy.is_ours("cloud_verify", None))
        self.assertFalse(sgpolicy.is_ours("cloud_verify:", None))

    def test_a_long_owner_stays_within_the_providers_limit(self):
        owner = "kylezh/" + "very-long-branch-name-" * 5
        tag = sgpolicy.describe(owner)
        self.assertLessEqual(len(tag), sgpolicy.MAX_DESCRIPTION)
        self.assertNotEqual(tag, sgpolicy.describe(owner + "x"))   # still unique
        self.assertEqual(tag, sgpolicy.describe(owner))            # still stable
        self.assertNotIn(" ", tag)

    def test_the_owner_lookup_that_fails_leaves_no_owner(self):
        # A caller that is already reporting a failure must not fail on this:
        # no owner means untagged rules, which nobody revokes.
        with mock.patch.object(sgpolicy, "current_owner", side_effect=OSError("no git here")):
            self.assertEqual(sgpolicy.owner_now(None), None)
        self.assertEqual(sgpolicy.owner_now("owner-a"), "owner-a")   # never looked up


class ThePlan(testkit.GuardedTestCase):
    """Who the launch reconcile adds, revokes and leaves alone."""

    def want(self, ips=(NEW,)):
        return sgpolicy.ssh_rules([22, 443, 2222], ips)

    def test_another_owners_rule_is_never_stale(self):
        # The incident, at the policy level: A's launch finds B's rule for an
        # address A does not use, and leaves it alone.
        change = sgpolicy.plan([rule(cidr=f"{OLD}/32", description="cloud_verify:owner-b")],
                               self.want(), "owner-a")
        self.assertEqual(change.stale, [])
        self.assertEqual(len(change.kept), 1)

    def test_this_owners_previous_rule_may_be_borrowed_and_is_kept(self):
        change = sgpolicy.plan([rule(cidr=f"{THIRD}/32", description="cloud_verify:owner-a"),
                                rule(cidr=f"{NEW}/32", description="cloud_verify:owner-a")],
                               self.want(), "owner-a")
        self.assertEqual(change.stale, [])
        self.assertEqual([r.cidr for r in change.kept], [f"{THIRD}/32", f"{NEW}/32"])

    def test_an_untagged_rule_is_never_revoked(self):
        change = sgpolicy.plan([rule(cidr=f"{OLD}/32"), rule(cidr=f"{THIRD}/32", description="cloud_verify")],
                               self.want(), "owner-a")
        self.assertEqual(change.stale, [])
        self.assertEqual(len(change.kept), 2)

    def test_a_rule_the_provider_cannot_name_back_is_kept(self):
        # No handle, no revoke: a rule the provider did not identify could not
        # be pointed at, and the change is not worth guessing at.
        change = sgpolicy.plan([rule(cidr=f"{THIRD}/32", description="cloud_verify:owner-a", ref=None)],
                               self.want(), "owner-a")
        self.assertEqual(change.stale, [])
        self.assertEqual(len(change.kept), 1)

    def test_an_address_another_owner_already_allowed_is_not_added_twice(self):
        # Every owner on this machine egresses from the same address: the rule
        # asked for twice is one rule, and asking again is a provider error.
        change = sgpolicy.plan([rule(cidr=f"{NEW}/32", description="cloud_verify:owner-b")], self.want(), "owner-a")
        self.assertEqual(set(change.missing), {("443", f"{NEW}/32"), ("2222", f"{NEW}/32")})

    def test_only_a_tcp_accept_rule_counts_as_present(self):
        change = sgpolicy.plan([rule(cidr=f"{NEW}/32", protocol="UDP"),
                                rule(cidr=f"{NEW}/32", action="DROP")], self.want(), "owner-a")
        self.assertIn(("22", f"{NEW}/32"), change.missing)
        # ... and neither of them is ours to revoke.
        self.assertEqual(change.stale, [])

    def test_a_world_open_rule_is_never_written(self):
        # Every rule this tool writes is a single address, so the guard is the
        # last line of defence: a want that would open the group is refused
        # rather than written.
        with self.assertRaises(cloud_common.InfraError) as ctx:
            sgpolicy.plan([], {("22", "0.0.0.0/0")}, "owner-a")
        self.assertIn("0.0.0.0/0", str(ctx.exception))
        with self.assertRaises(cloud_common.InfraError):
            sgpolicy.plan([], {("22", "::/0")}, "owner-a")

    def test_a_world_open_rule_that_is_already_there_is_kept_and_reported(self):
        change = sgpolicy.plan([rule(cidr="0.0.0.0/0")], self.want(), "owner-a")
        self.assertEqual(change.stale, [])
        self.assertEqual([r.cidr for r in change.world], ["0.0.0.0/0"])
        self.assertIn("0.0.0.0/0", sgpolicy.world_note(change))
        self.assertIn("only an idle-account coordinator prune may revoke tagged rules", sgpolicy.world_note(change))

    def test_the_notes_name_what_was_left_alone(self):
        change = sgpolicy.plan([rule(description="cloud_verify:owner-b"), rule()], self.want(), "owner-a")
        self.assertIn("kept 2 existing ingress rule(s)", sgpolicy.kept_note(change))
        self.assertIn("1 of them with no owner tag", sgpolicy.kept_note(change))
        self.assertEqual(sgpolicy.world_note(change), None)
        self.assertEqual(sgpolicy.kept_note(sgpolicy.plan([], self.want(), "owner-a")), None)

    def test_the_ports_are_spelled_the_way_the_provider_spells_them(self):
        self.assertEqual(sgpolicy.ssh_rules([22, 443], [NEW]),
                         {("22", f"{NEW}/32"), ("443", f"{NEW}/32")})
        self.assertEqual(sgpolicy.ssh_rules([22], [NEW], fmt=lambda p: f"{p}/{p}"), {("22/22", f"{NEW}/32")})


class TheRepairPath(testkit.GuardedTestCase):
    """The other half of the ssh story (cloud/allowlist.py): a rule that the
    launch reconcile left behind is added back by the transports, add only -
    and a dead instance is still reported as dead, not repaired."""

    class Provider:
        name, ssh_user, ssh_ports = "tencent", "ubuntu", [22, 443, 2222]

        def __init__(self, state):
            self.state, self.writes = state, []

        def instance_state(self, m):
            return self.state

        def public_ips(self, proxy):
            return [NEW]

        def allow_ssh(self, m, ips):
            self.writes.append(list(ips))
            return f"security group sg-1: ssh allowed from {', '.join(ips)} (3 new rule(s))"

        def allow_hint(self, m, ips):
            return "tccli vpc CreateSecurityGroupPolicies --SecurityGroupId sg-1"

    def setUp(self):
        quiet = mock.patch.object(cloud_allowlist, "log")
        quiet.start()
        self.addCleanup(quiet.stop)
        quiet = mock.patch.object(cloud_allowlist, "warning")
        quiet.start()
        self.addCleanup(quiet.stop)

    def access(self, state):
        provider = self.Provider(state)
        m = mock.Mock(id="ins-1", proxy=None, region="ap-shanghai")
        return provider, cloud_allowlist.SshAccess(provider, m, [OLD], sleep=lambda secs: None)

    def test_a_running_instance_gets_the_current_address_added(self):
        provider, access = self.access("running")
        self.assertTrue(access.repair("ssh: connect to host x port 22: Operation timed out"))
        self.assertEqual(provider.writes, [[NEW]])

    def test_a_dead_instance_is_reported_as_dead_and_not_repaired(self):
        for state in ("gone", "stopped"):
            with self.subTest(state=state):
                provider, access = self.access(state)
                self.assertFalse(access.repair("ssh: connect to host x port 22: Operation timed out"))
                self.assertEqual(provider.writes, [])


if __name__ == "__main__":
    unittest.main()
