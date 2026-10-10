"""Atomic JSON replacement and explicit locked read-modify-write transactions.

Atomic replacement protects readers from partial bytes, not from lost updates.
A transaction holds a stable sidecar lock through read, callback and replacement;
all concurrent writers must use that same lock (never unlink a live lock file).
Formatting belongs to callers. Invalid JSON and callback/write failures propagate.
"""
import contextlib
import fcntl
import json
import os
from pathlib import Path
import tempfile


@contextlib.contextmanager
def file_lock(path):
    """Serialize cooperating writers on a stable, caller-selected lock path."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        yield


def atomic_json_replace(path, value, *, indent=1, trailing_newline=False, **options):
    """Publish a complete JSON value using a unique temporary in the same directory.

    fsync the temporary before replacement. This is atomic publication, not a
    power-loss durability guarantee for the containing directory.
    """
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False) as output:
            temporary = output.name
            json.dump(value, output, indent=indent, **options)
            if trailing_newline:
                output.write('\n')
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)


def atomic_json_update(path, update, **options):
    """Read/modify/replace without a lock; caller must serialize competing updates.

    A missing file starts as {}; callbacks may mutate in place and return None
    or return a replacement value. Return the published value.
    """
    try:
        with open(path) as source:
            data = json.load(source)
    except FileNotFoundError:
        data = {}
    result = update(data)
    value = data if result is None else result
    atomic_json_replace(path, value, **options)
    return value


def locked_json_update(path, update, *, lock_path=None, **options):
    """Hold the sidecar lock for the entire read/modify/replace transaction."""
    with file_lock(lock_path if lock_path is not None else str(path) + '.lock'):
        return atomic_json_update(path, update, **options)
