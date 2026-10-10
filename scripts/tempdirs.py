"""Attributable temporary directories; completion never guesses another worker's owner.

The marker ``.tlx-temp-owner.json`` (``{"root": <checkout>, "pid": <pid>}``) is written
by :func:`mkdtemp` / :class:`TemporaryDirectory` here and by ``tlx_testkit::TempDir``
(``crates/tlx-testkit/src/lib.rs``). ``problems`` reports a leftover directory only when
the marker names the checked checkout, so a worker never claims another worker's dirs.
"""
import json
from pathlib import Path
import os
import shlex
import tempfile

ROOT = Path(__file__).resolve().parents[1]
MARKER = '.tlx-temp-owner.json'


def owner_metadata(owner_root):
    """The marker body naming the resolved ``owner_root`` as the owner."""
    return {'root': str(Path(owner_root).resolve()), 'pid': os.getpid()}


def mark(directory, owner_root):
    (Path(directory) / MARKER).write_text(json.dumps(owner_metadata(owner_root)))
    return directory


def owner(directory):
    """The metadata ``directory`` records, or None when it carries no usable
    marker (absent, unreadable or malformed)."""
    try:
        metadata = json.loads((Path(directory) / MARKER).read_text())
    except (OSError, ValueError):
        return None
    return metadata if isinstance(metadata, dict) else None


def mkdtemp(*args, owner_root=ROOT, **kwargs):
    directory = tempfile.mkdtemp(*args, **kwargs)
    try:
        return mark(directory, owner_root)
    except BaseException:
        import shutil
        shutil.rmtree(directory)
        raise


class TemporaryDirectory(tempfile.TemporaryDirectory):
    def __init__(self, *args, owner_root=ROOT, **kwargs):
        super().__init__(*args, **kwargs)
        try:
            mark(self.name, owner_root)
        except BaseException:
            self.cleanup()
            raise


def candidates(root, tmp_root=None):
    """Every temporary directory this checkout could be blamed for, right now.

    The same scan ``problems`` reports from, in the same order; ``snapshot`` is
    this as a set, and it exists so a caller can hold one and ask later which of
    these it can be blamed for.
    """
    root = Path(root).resolve()
    found = []
    target = root / 'target'
    # Legacy directories inside this worktree are attributable without a marker.
    if target.is_dir():
        for directory in target.glob('tlx-*'):
            if (directory.is_dir() and not directory.is_symlink()
                    and (directory.name.startswith(('tlx-mutants', 'tlx-gate-specs'))
                         or (directory / MARKER).is_file())):
                found.append(directory)
    temporary = Path(tmp_root or tempfile.gettempdir())
    candidates_ = temporary.iterdir() if temporary.is_dir() else ()
    for directory in candidates_:
        if not directory.name.startswith(('tlx-', 'tlc-')) or not directory.is_dir() or directory.is_symlink():
            continue
        metadata = owner(directory)  # Unmarked or malformed markers have no proven owner.
        if metadata is not None and metadata.get('root') == str(root):
            found.append(directory)
    return found


def snapshot(root, tmp_root=None):
    """The directories ``problems`` would consider, as of now.

    Take it before the work that starts tools of its own and pass it back as
    ``problems(..., seen=...)``: what appeared afterwards belongs to a run that
    is still going, not to whatever the caller is checking (H-03 item 5).
    """
    return set(candidates(root, tmp_root))


def problems(root, tmp_root=None, *, seen=None):
    """Leftover directories of ``root``, each with its cleanup command.

    ``seen`` is an earlier ``snapshot``: with it, only a directory that already
    existed when the snapshot was taken is reported. A directory created after
    that point belongs to a tool that is still running - a spectest's private
    binary under ``target/``, a gate's private spec copy - and reporting it made
    two runs of the same completion check disagree: the first blamed the worker
    for the tool in front of it, the second (after the tool had cleaned up)
    passed (H-03 item 5). Without ``seen`` every owned directory is reported,
    which is the module's own contract and what its tests drive.

    A directory whose owner marker names a pid is reported with that pid, so the
    reader can tell which process wrote it; ownership is never inferred from a
    missing or malformed marker (``root`` is the only claim a directory makes).
    """
    root = Path(root).resolve()
    report = []
    for directory in candidates(root, tmp_root):
        if seen is not None and directory not in seen:
            continue
        metadata = owner(directory)
        pid = metadata.get('pid') if metadata else None
        where = f' (owner pid {pid})' if isinstance(pid, int) else ''
        report.append(f'leftover temporary directory {directory}{where}; '
                      f'clean with: rm -rf -- {shlex.quote(str(directory))}')
    return sorted(report)
