"""Git execution without caller policy: retain stdout, stderr and exit status.

Adapters own decoding, whitespace and rejection policy. Checked failures use
CalledProcessError with the same bytes/text payloads as successful results.
"""
import subprocess


def run(*args, cwd=None, text=True, check=False, input=None, capture_output=True):
    """Return a CompletedProcess; never strip output or reinterpret an exit code."""
    return subprocess.run(["git", *args], cwd=cwd, text=text, check=check,
                          input=input, capture_output=capture_output)
