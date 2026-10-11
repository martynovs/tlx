"""Owned child lifecycle shared by drivers and gate adapters.

Only construction launches a child: this API never adopts a PID observed by ps.
Capture policies (pipes, streaming or wait4-backed files) share this ownership;
process-group cancellation is an explicit opt-in used by gates and benchmarks.
No process-wide usage deltas or disk sampling belong here.
"""
import contextlib
import os
import signal
import subprocess
import tempfile
import threading
import time
from dataclasses import dataclass


class OwnedChild:
    """A child launched here, with one owner responsible for draining and reaping.

    A new session gives the owner authority over this child's process group too.
    Group signals deliberately remain valid after the leader exits, since a
    descendant may still hold an output pipe. The caller defines grace policy.
    """

    def __init__(self, command, *, start_new_session=False, **kwargs):
        if isinstance(command, int):
            raise TypeError("cannot adopt a foreign PID or Popen; launch an owned child")
        self._group = start_new_session
        self._process = subprocess.Popen(command, start_new_session=start_new_session, **kwargs)

    @property
    def pid(self):
        return self._process.pid

    @property
    def stdout(self):
        return self._process.stdout

    @property
    def returncode(self):
        return self._process.returncode

    def poll(self):
        return self._process.poll()

    def wait(self, timeout=None):
        return self._process.wait(timeout=timeout)

    def communicate(self, timeout=None):
        return self._process.communicate(timeout=timeout)

    def kill(self):
        self._process.kill()

    def terminate(self):
        self._process.terminate()

    def signal_group(self, sig):
        if not self._group:
            raise ValueError("child was not launched in an owned process group")
        os.killpg(self.pid, sig)

    def wait_usage(self, timeout):
        """Reap exactly this child with wait4; never poll/wait before this call.

        Regular Popen waits would discard its usage. At the deadline kill this
        unreaped child (and its group if owned), then collect its status and usage.
        """
        deadline = time.monotonic() + timeout if timeout is not None else None
        expired = False
        while True:
            child, status, usage = os.wait4(self.pid, os.WNOHANG if deadline is not None else 0)
            if child:
                break
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                expired = True
                if self._group:
                    with contextlib.suppress(ProcessLookupError):
                        self.signal_group(signal.SIGKILL)
                else:
                    os.kill(self.pid, signal.SIGKILL)
                _, status, usage = os.wait4(self.pid, 0)
                break
            time.sleep(min(.001, remaining))
        self._process.returncode = os.waitstatus_to_exitcode(status)
        return usage, expired

    def __enter__(self):
        self._process.__enter__()
        return self

    def __exit__(self, *error):
        return self._process.__exit__(*error)


@dataclass
class ProcResult:
    stdout: str
    stderr: str
    returncode: int
    wall: float
    timed_out: bool = False
    rusage: object = None


def capture(cmd, timeout=None, *, cwd=None, env=None, rusage=False, grace=0, raise_timeout=True,
             on_line=None, stop_when=None, errors=None, pass_fds=(), start_new_session=False):
    """Capture and reap one child; optionally measure that child's wait4 usage.

    grace > 0 sends TERM before KILL (tlx spill cleanup); zero preserves the
    immediate-kill policy used by TLC and the measured benchmark runs.
    start_new_session owns a separate process group: cancellation and scope exit
    kill its remaining members, even if the direct child has already exited.
    No observed external PID or caller process group is ever adopted.
    """
    if rusage and on_line is not None:
        raise ValueError("rusage and streaming are separate capture modes")
    started = time.time()
    timed_out = threading.Event()
    usage = None
    with contextlib.ExitStack() as stack:
        output = stack.enter_context(tempfile.TemporaryFile("w+")) if rusage else subprocess.PIPE
        process = stack.enter_context(OwnedChild(cmd, cwd=cwd, env=env, stdout=output,
                                      start_new_session=start_new_session,
                                      stderr=subprocess.STDOUT if rusage or on_line is not None else subprocess.PIPE,
                                      text=True, errors=errors, pass_fds=pass_fds, bufsize=1 if on_line is not None else -1))
        def kill():
            if start_new_session:
                with contextlib.suppress(ProcessLookupError):
                    process.signal_group(signal.SIGKILL)
            else:
                process.kill()

        def terminate():
            if start_new_session:
                with contextlib.suppress(ProcessLookupError):
                    process.signal_group(signal.SIGTERM)
            else:
                process.terminate()

        try:
            if on_line is not None:
                lines, failures = [], []
                def read():
                    try:
                        for line in process.stdout:
                            lines.append(line)
                            on_line(line)
                    except Exception as error:
                        failures.append(error)
                reader = threading.Thread(target=read, daemon=True)
                reader.start()
                try:
                    while reader.is_alive() or process.poll() is None:
                        if failures:
                            break
                        if timeout is not None and time.time() - started >= timeout:
                            timed_out.set()
                            break
                        if stop_when is not None and stop_when():
                            break
                        reader.join(.05)
                finally:
                    if start_new_session or process.poll() is None:
                        kill()
                    process.wait()
                    reader.join()
                if failures:
                    raise failures[0]
                stdout, stderr, wall = "".join(lines), "", time.time() - started
            elif rusage:
                usage, expired = process.wait_usage(timeout)
                if expired:
                    timed_out.set()
                wall = time.time() - started
                output.seek(0)
                stdout, stderr = output.read(), ""
            else:
                try:
                    stdout, stderr = process.communicate(timeout=timeout)
                except subprocess.TimeoutExpired:
                    timed_out.set()
                    if grace:
                        terminate()
                        try:
                            stdout, stderr = process.communicate(timeout=grace)
                        except subprocess.TimeoutExpired:
                            kill()
                            stdout, stderr = process.communicate()
                    else:
                        kill()
                        stdout, stderr = process.communicate()
                wall = time.time() - started
        finally:
            if start_new_session or process.returncode is None:
                kill()
            if process.returncode is None:
                process.communicate()
        if timed_out.is_set() and raise_timeout:
            raise subprocess.TimeoutExpired(cmd, timeout, output=stdout, stderr=stderr)
        return ProcResult(stdout, stderr, process.returncode, wall, timed_out.is_set(), usage)

