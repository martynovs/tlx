"""Bounded recovery of post-boot SSH, rsync and git-over-SSH failures.

Transport stderr is always retained by Machine.run, including visible setup
steps. A running provider state is evidence to try add-only allowlist repair,
not proof that the network works. Each idempotent step has an attempt/deadline
bound, while provider calls share a locked, recharging rate budget across the
parallel setup and periodic result-copy threads. The final failed result carries
its own diagnosis so another thread cannot overwrite the STOP's explanation.
"""
import os
import re
import time
import threading

from cloud.cloud_common import log, warning, network_deadline, over_ssh, TERMINATE_RESERVE_SECS
from cloud import deadline as request_deadline

# Provider calls are rate-limited across all steps/threads of one instance.
# A later outage gets a fresh window; an individual step is still bounded.
MAX_ROUNDS = 3
REPAIR_RECHARGE_SECS = 900
MAX_STEP_ATTEMPTS = 4
# Wait after a successful re-apply: a provider takes a moment to commit the
# rule, and a retry that leaves immediately is the one that tends to fail.
SETTLE_SECS = 5

# What an unreachable host looks like on stderr: the kernel, ssh or rsync
# saying the packets did not get there. None of these is the remote side
# answering and turning us away.
UNREACHABLE_HINTS = (
    "connection timed out", "connect timed out", "operation timed out",
    "connection refused", "no route to host", "network is unreachable",
    "host is down", "connection closed by", "connection reset by peer",
    "connection unexpectedly closed", "kex_exchange_identification",
    "ssh_exchange_identification", "banner exchange", "remote end closed connection",
    "broken pipe", "timed out waiting for input", "did not receive identification string",
)
# Reaching the host and being refused is a different problem (a key pair that
# was replaced, a user that does not exist): the allowlist is not the cause.
REACHED_HINTS = ("permission denied", "no supported authentication methods",
                 "host key verification failed", "too many authentication failures")


def output(p):
    """stdout and stderr of a completed process as one string. Either may be
    bytes (the transfers read them) or text (`Machine.ssh`), and either may be
    absent, so they are decoded one at a time."""
    parts = []
    for stream in (p.stdout, p.stderr):
        if isinstance(stream, bytes):
            parts.append(stream.decode(errors="replace"))
        elif stream:
            parts.append(stream)
    return "".join(parts)


def unreachable(p, cmd):
    """The line that shows why `cmd` could not reach the instance, or None
    when this is not a transport failure of an ssh command."""
    if p.returncode == 0 or not over_ssh(cmd):
        return None
    lines = [line.strip() for line in output(p).splitlines() if line.strip()]
    low = " ".join(lines).lower()
    if any(hint in low for hint in REACHED_HINTS):
        return None
    for line in lines:
        if (re.fullmatch(r"Timeout, server .+ not responding\.", line)
                or any(hint in line.lower() for hint in UNREACHABLE_HINTS)):
            return line[:200]
    return None


class SshAccess:
    """The allowlist state of one launched machine, and its bounded recovery.

    `ips` are the addresses the security group was written with at launch.
    `sleep` and `now` are injectable for tests. `recharge=None` retains a
    lifetime hard bound; otherwise rounds recharge after a fixed rate window."""

    def __init__(self, provider, m, ips, *, rounds=MAX_ROUNDS, settle=SETTLE_SECS, sleep=None,
                 now=None, recharge=REPAIR_RECHARGE_SECS):
        self.provider, self.m = provider, m
        self.written = list(ips)     # the addresses the rules were written with
        self.max_rounds, self.settle = rounds, settle
        self.rounds = 0              # provider rounds spent in the current rate window
        self.window_at = None
        self.recharge = recharge
        self.last_state = "not queried"
        self.repair_note = ""
        self._lock = threading.RLock()
        self.said = set()            # diagnoses already printed (a retry loop repeats the failure)
        self.state_error = None      # why the last state read failed, if it did
        self._sleep = time.sleep if sleep is None else sleep
        self._now = time.monotonic if now is None else now

    def say(self, kind, message):
        """Warn once per `kind`: a caller's retry loop hands us the same failure
        again and again, and one actionable message is the point of it."""
        if kind not in self.said:
            self.said.add(kind)
            warning(message)

    def run(self, attempt, cmd, step=None, deadline=None):
        """Replay an idempotent transport step after repair, within both bounds.

        `attempt(remaining_seconds)` applies the timeout to the subprocess.
        Each step gets at most MAX_STEP_ATTEMPTS attempts and the network
        deadline, which reserves cleanup time before the dead-man's switch.
        Provider repairs share a separate, locked rate budget across threads.
        The detached job starter must itself be idempotent: reconnecting never
        means starting a second benchmark.
        """
        label = step or (str(cmd[-1])[:120] if cmd else "transport")
        until = network_deadline(getattr(self.m, "dead_at", None), monotonic=self._now)
        if deadline is not None:
            until = min(until, deadline)
        repairs = attempts = 0
        for tries in range(1, MAX_STEP_ATTEMPTS + 1):
            if tries > 1 and self._now() >= until:
                stop = "step deadline exhausted"
                break
            attempts += 1
            p = attempt(max(0.0, until - self._now()))
            reason = unreachable(p, cmd)
            if reason is None:
                return p
            if tries == MAX_STEP_ATTEMPTS or self._now() + self.settle >= until:
                stop = "step retry budget exhausted"
                break
            if not self.repair(reason, deadline=until):
                stop = "allowlist repair unavailable"
                break
            repairs += 1
            if self._now() >= until:
                stop = "step deadline exhausted after repair"
                break
            log(f"re-running {label} on {self.m.id}: ssh transport failed ({reason}); "
                f"allowlist repaired, attempt {tries + 1}/{MAX_STEP_ATTEMPTS}")
        p.ssh_diagnosis = (f"provider state: {self.last_state}; step: {label}; attempts: {attempts}; "
                           f"repairs: {repairs}; {stop}; {self.repair_note}; last transport error: {reason}")
        return p

    def repair(self, reason, deadline=None):
        """One bounded diagnosis-or-repair round; True when the allowlist was
        re-applied for this machine's current public address and the command is
        worth running again."""
        self.repair_note = ""
        if deadline is None:
            with self._lock:
                return self._repair(reason)
        left = deadline - self._now()
        if left <= TERMINATE_RESERVE_SECS:
            self.repair_note = "repair skipped: insufficient time beyond the cleanup reserve"
            self.say("cleanup", self.repair_note)
            return False
        if not self._lock.acquire(timeout=left - TERMINATE_RESERVE_SECS):
            self.repair_note = "repair lock deadline expired; preserving cleanup reserve"
            return False
        try:
            # Preserve a second reserve while diagnosing: no request or lock
            # wait may spend the time reserved for releasing the instance.
            with request_deadline.bound(deadline - TERMINATE_RESERVE_SECS, now=self._now):
                request_deadline.remaining()
                return self._repair(reason)
        except TimeoutError as error:
            self.repair_note = str(error)
            self.say("deadline", self.repair_note)
            return False
        finally:
            self._lock.release()

    def _repair(self, reason):
        instant = self._now()
        if (self.recharge is not None and self.window_at is not None
                and instant - self.window_at >= self.recharge):
            self.rounds = 0
            self.window_at = None
            self.said.clear()
        if self.rounds >= self.max_rounds:
            self.say("budget", f"cannot reach {self.m.id} over ssh ({reason}): the ssh allowlist was already "
                               f"diagnosed {self.rounds} times in this rate window; not spending another "
                               f"provider call. If the address moved again, add it with:\n    {self.hint()}")
            return False
        if self.window_at is None:
            self.window_at = instant
        self.rounds += 1
        state = self.state()
        if state is None:
            self.say("unknown", f"cannot reach {self.m.id} over ssh ({reason}) and cannot read its state from the "
                                f"provider either ({self.state_error}): not changing the ssh allowlist on a guess. "
                                f"If the instance is up, add this machine's current address with:\n    {self.hint()}")
            return False
        if state != "running":
            self.say(f"state:{state}", f"cannot reach {self.m.id} over ssh ({reason}), but the provider reports it "
                                       f"{state}: the instance, not the ssh allowlist, is the problem - not "
                                       f"re-applying the allowlist")
            return False
        try:
            request_deadline.remaining()
            ips = self.provider.public_ips(self.m.proxy)
        except Exception as e:  # noqa: BLE001 - the caller's failure is already in hand
            self.say("address", f"cannot reach {self.m.id} over ssh ({reason}) and cannot read this machine's "
                                f"public address ({e}); add the current address to the ssh allowlist by hand "
                                f"with:\n    {self.hint()}")
            return False
        warning(f"cannot reach {self.m.id} over ssh ({reason}), but the instance is running at the provider: this "
                f"may be the operator-IP allowlist or a network outage. This machine is now {', '.join(ips)}; the ssh rules were written "
                f"with {', '.join(self.written)}. Re-applying it ({self.rounds} of {self.max_rounds}); by hand "
                f"that is:\n    {self.hint(ips)}")
        try:
            request_deadline.remaining()
            note = self.provider.allow_ssh(self.m, ips)
        except Exception as e:  # noqa: BLE001 - reported with the command that fixes it
            self.say("write", f"could not add {', '.join(ips)} to the ssh allowlist of {self.m.id} ({e}); add it "
                              f"by hand with:\n    {self.hint(ips)}")
            return False
        self.written = list(ips)
        log(f"{note}; waiting for ssh rules to settle on {self.m.id}")
        self._sleep(min(self.settle, request_deadline.remaining(self.settle)))
        return True

    def state(self):
        """The provider's view of this machine ('running', 'gone', 'stopped')
        or None when it cannot be read (the reason is left in `state_error`)."""
        self.state_error = None
        try:
            self.last_state = self.provider.instance_state(self.m)
            return self.last_state
        except Exception as e:  # noqa: BLE001 - the caller reports it with the rest of the diagnosis
            self.state_error = e
            self.last_state = f"unknown ({e})"
            return None

    def hint(self, ips=None):
        """The exact provider call that would allow `ips` (this machine's
        launch-time address by default); never raises."""
        try:
            return self.provider.allow_hint(self.m, ips or self.written)
        except Exception as e:  # noqa: BLE001 - a hint must not hide the failure it explains
            return f"(this provider cannot build the command here: {e})"
