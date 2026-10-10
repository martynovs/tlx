"""One cloud session: its phases, and the single cleanup that ends it.

`Session.run` owns the run's cleanup, and with it the only `finally` that
touches what a run can leave behind. A phase -- boot, remote setup, the passes
-- may raise anywhere, and the cleanup then releases the instance, verifies
that nothing this launch created is still running and writes the receipt the
results are bound to (HARNESS-06, HARNESS-09). No phase releases or receipts on
its own, so a phase added later cannot skip either, and there is no second
place to keep in step with the first.

(The other `finally` of a session is `run_pass`'s: it stops and joins a pass's
copier thread, which holds no instance, ledger row or receipt. `run`'s own
`finally` in the entrypoint restores the signal handlers it installed.)

The phases are `cloud/remote.py`; the plan they execute is `cloud/setup.py`. A
phase reaches the entrypoint only through `Hooks`, so neither module imports the
CLI or a test helper (HARNESS-03).
"""
import dataclasses
import os
import signal
import time
from typing import Any, Callable

from cloud import remote
from cloud.cloud_common import CloudStop, INFRA_FAIL, Interrupted, active_launch_leftovers, cloud_receipt, fmt_secs, log, retry_network, terminate_until_gone, warning, write_receipt

HANDLED_SIGNALS = (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)


@dataclasses.dataclass(frozen=True)
class Hooks:
    """The entrypoint functions the phases call back into.

    `launch` reserves and launches one offer (the ledger admission checkpoint
    lives with it) and `key` ensures this machine's ssh key exists. Both are
    read when the session is built rather than bound at import, so a unit test
    that patches the entrypoint's own name -- `cloud_verify.launch_with_quota_wait`
    or `cloud_verify.ensure_local_key` -- keeps patching what the boot phase
    calls.
    """
    launch: Callable
    key: Callable


@dataclasses.dataclass
class Session:
    """One launch: the plan it executes, the machine it launched, what became of it.

    The mutable state of a run lives here rather than in local variables of the
    phases, so the cleanup can report it after a phase raised: `phases` is the
    per-phase timing the receipt states, `then_results` the per-`--then` exits,
    `used_regions`/`launched_ids` what this launch created (and therefore what
    the leftover check is allowed to judge), and `rc` the worst exit status so
    far -- an infrastructure failure once a release failed, whatever the passes
    said.
    """
    a: Any
    plan: Any
    cancel: Any
    hooks: Hooks
    phases: dict = dataclasses.field(default_factory=dict)
    then_results: list = dataclasses.field(default_factory=list)
    machine: Any = None
    used_regions: set = dataclasses.field(default_factory=set)
    launched_ids: set = dataclasses.field(default_factory=set)
    rc: int = INFRA_FAIL
    fetched_at: float | None = None

    @property
    def local_dir(self):
        return self.plan.local_dir

    def run(self):
        """Boot, set up, run the passes, fetch -- and always clean up.

        A stop is a normal end of the session, not a traceback: an interrupt, a
        cancelled launch or a refusal is logged and turned into its exit status,
        and anything unexpected becomes an infrastructure failure. `run` is the
        only place with a `finally`, so every one of those paths releases the
        instance and leaves its receipt.
        """
        try:
            remote.boot(self)
            remote.prepare(self)
            remote.passes(self)
        except Interrupted as e:
            log(f"STOP: interrupted ({e})")
        except KeyboardInterrupt:
            log("STOP: interrupted (Ctrl-C)")
        except CloudStop as e:
            self.rc = e.code
            log(str(e))
        except Exception as e:  # noqa: BLE001 - report, then release in cleanup
            error = CloudStop(f"infrastructure failure: {type(e).__name__}: {e}")
            self.rc = error.code
            log(str(error))
        finally:
            self.cleanup()
        return self.rc

    def cleanup(self):
        """Release the instance, check what this launch left, write the receipt.

        Nothing may interrupt it: the signals are ignored for its duration, a
        release that fails is reported and never raised out of here (it turns
        the run into an infrastructure failure instead), and the receipt is
        written last so its `verify_exit` is the run's final status (HARNESS-06).
        The leftover check judges only instances this launch created -- a
        concurrent owned launch is valid and is not this run's leak.
        """
        for s in HANDLED_SIGNALS:
            signal.signal(s, signal.SIG_IGN)
        a, plan, m = self.a, self.plan, self.machine
        provider, cur = plan.provider, plan.currency
        live = bool(m and provider.find_launched(m))
        if live:
            if a.keep_instance:
                warning(f"instance kept: {m.id} at {m.ip} (ssh: {' '.join(m.ssh_cmd())}); release with:\n"
                        f"    {provider.terminate_hint(m)}\n"
                        f"  (the dead-man's switch ends it {a.max_minutes} min after the launch)")
            elif not m.terminated:
                m.t_end = time.time()
                try:
                    m.terminated = terminate_until_gone(provider, m)
                    if m.terminated and getattr(m, "ledger_time", None):
                        plan.ledger.release(plan.owner, m.ledger_time, m.id)
                except Exception as e:  # noqa: BLE001 - report, never raise out of cleanup
                    warning(f"terminate {m.id} failed: {type(e).__name__}: {e}")
                if not m.terminated:
                    self.rc = INFRA_FAIL
                    print(f"STOP: COULD NOT RELEASE {m.id} in {m.region}: run\n    {provider.terminate_hint(m)}\n", flush=True)
            print(f"\ncloud: {m.offer.itype} in {m.offer.region} ({m.offer.zone}), {m.offer.market} {m.offer.price:.4f} {cur}/h, "
                  f"wall {fmt_secs(m.wall())} ("
                  + ", ".join(f"{k} {fmt_secs(v)}" for k, v in self.phases.items() if isinstance(v, float))
                  + f"), estimated cost {m.cost():.3f} {cur}" + (" (still running!)" if a.keep_instance else ""), flush=True)
        if self.used_regions and not a.keep_instance:
            try:
                left = retry_network(lambda: provider.leftovers(sorted(self.used_regions), owner=plan.owner),
                                     "list owned leftovers")
                if active_launch_leftovers(left, self.launched_ids):
                    self.rc = INFRA_FAIL
                    log("STOP: this launch left cloud instances active after cleanup")
                log("tagged instances left: " + (", ".join(f"{r} {i} {s}" for r, i, s in left) if left else "none"))
            except Exception as e:  # noqa: BLE001 - informational
                self.rc = INFRA_FAIL
                log(f"STOP: cannot verify owned leftover instances: {e}")
        # (HARNESS-06) Every invocation that produced a result directory leaves
        # a receipt, whether or not a machine came up and whether the run
        # failed, so a consumer binds the results to this run instead of
        # guessing by mtime, and a failure that produced no results is recorded
        # as failure evidence rather than silently leaving nothing behind.
        if os.path.isdir(self.local_dir):
            write_receipt(self.local_dir, cloud_receipt(a, plan.sha, self.rc, phases=self.phases, rust=plan.rust,
                                                        crates=plan.crates, machine=m if live else None, bins=plan.bins,
                                                        then_results=self.then_results, fetched_at=self.fetched_at,
                                                        currency=cur))
