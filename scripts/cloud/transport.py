"""Bounded JSON CLI transport; provider argv and refusal policy stay with providers."""
import json
import subprocess

from cloud import deadline
from cloud.cloud_common import AccountError, InfraError, no_proxy_env

CLI_TIMEOUT = 120


def cli_env(credential_prefixes):
    """Prevent ambient proxies and credentials from overriding the named profile."""
    return {k: v for k, v in no_proxy_env().items() if not k.startswith(credential_prefixes)}


def capture_json(cmd, *, env, missing, error):
    """Capture and reap one owned CLI child; decode success, delegate provider errors.

    `error` builds the provider's exception from combined stdout/stderr. Empty
    success is an empty object; malformed success JSON retains JSONDecodeError.
    subprocess.run kills and waits for the child when its 120s deadline expires.
    """
    try:
        p = subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=deadline.remaining(CLI_TIMEOUT))
    except FileNotFoundError as e:
        raise AccountError(missing) from e
    except subprocess.TimeoutExpired as e:
        raise InfraError(f"{' '.join(cmd[:3])} timed out") from e
    if p.returncode != 0:
        raise error(p.stdout + p.stderr)
    return json.loads(p.stdout) if p.stdout.strip() else {}
