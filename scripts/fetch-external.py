#!/usr/bin/env python3
"""Fetch the external TLA+ corpora of corpus/external.json into third_party/external/.

usage:
  scripts/fetch-external.py [--repos NAME,...] [--force] [--list]
  scripts/fetch-external.py --overlay-only            # apply corpus/external/<name>/
                                                      # over an already-installed, uploaded tree (cloud)

Nothing is vendored: every repository is downloaded at its pinned commit
(`repos.<name>.commit`) and unpacked under third_party/external/<name>/ (git-ignored).
Repositories without a license are only ever fetched like this, never copied
into the tree. The files under corpus/external/<name>/ (our own cfg files and
wrapper modules) are copied over the unpacked tree each run, at the same
relative paths.

A repository entry has:
  repo, commit      GitHub owner/name and the pinned commit
  root              only this directory of the repository is unpacked, and
                    becomes the top of third_party/external/<name>/ (default: all)
  fetch             "tarball" (default: one codeload tarball, optionally checked
                    against `sha256`; with `include` only those paths are
                    unpacked) or "files" (the files below `include`,
                    directory by directory through the GitHub contents API and
                    raw.githubusercontent.com: for repositories whose tarball
                    is huge). GITHUB_TOKEN, if set, raises the API rate limit.
  include           paths (relative to `root`) to unpack, or with "files" to
                    download; a directory means everything below it
  license, attribution, redistributable   documentation; see docs/testing.md

Without --repos every repository of the manifest is fetched (not the `suites`
entries, which have the same format and are named explicitly). A repository that
is already at its pinned commit is not downloaded again (--force does).
"""
import argparse, hashlib, io, json, os, shutil, sys, tarfile, urllib.error, urllib.parse, urllib.request

import dependencies
from tlxlib import ROOT, TP

MANIFEST = os.path.join(ROOT, "corpus", "external.json")
OVERLAY = os.path.join(ROOT, "corpus", "external")
# Real path: in a worktree third_party is a symlink to the main checkout.
DEST = os.path.join(os.path.realpath(TP), "external")
MARKER = ".fetched"


def load_manifest(path=MANIFEST):
    with open(path) as source:
        return json.load(source)


def request(url, retries=3):
    headers = {"User-Agent": "tlx-fetch-external"}
    if token := os.environ.get("GITHUB_TOKEN"):
        if "api.github.com" in url:
            headers["Authorization"] = f"Bearer {token}"
    last = None
    for _ in range(retries):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=120) as response:
                return response.read()
        except (urllib.error.URLError, TimeoutError) as e:
            last = e
            if isinstance(e, urllib.error.HTTPError) and e.code in (403, 404):
                break
    raise SystemExit(f"fetch-external: {url}: {last}")


def safe_member(name):
    """The member's path with no way out of the destination (None: skip it)."""
    parts = [p for p in name.split("/") if p]
    if not parts or any(p == ".." for p in parts) or os.path.isabs(name):
        return None
    return parts


def unpack_tarball(data, dest, root, include=()):
    """Unpack the regular files below `root` of a GitHub tarball (its single
    top directory stripped) into `dest`, only those below one of the paths of
    `include` if given; returns the number of files."""
    prefix = [p for p in (root or "").split("/") if p]
    wanted = [[p for p in inc.split("/") if p] for inc in include]
    count = 0
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        for member in tar:
            parts = safe_member(member.name)
            if parts is None or not member.isfile():
                continue  # (links and devices are never unpacked)
            parts = parts[1:]
            if parts[: len(prefix)] != prefix or len(parts) == len(prefix):
                continue
            rel = parts[len(prefix):]
            if wanted and not any(rel[: len(w)] == w for w in wanted):
                continue
            target = os.path.join(dest, *rel)
            os.makedirs(os.path.dirname(target), exist_ok=True)
            with tar.extractfile(member) as src, open(target, "wb") as out:
                shutil.copyfileobj(src, out)
            os.chmod(target, 0o644)
            count += 1
    return count


def list_files(repo, commit, path):
    """(path, raw URL) of every file below `path` of the repository."""
    url = f"https://api.github.com/repos/{repo}/contents/{urllib.parse.quote(path)}?ref={commit}"
    entries = json.loads(request(url))
    if isinstance(entries, dict):
        entries = [entries]
    for e in entries:
        if e["type"] == "dir":
            yield from list_files(repo, commit, e["path"])
        elif e["type"] == "file":
            yield e["path"], f"https://raw.githubusercontent.com/{repo}/{commit}/{urllib.parse.quote(e['path'])}"


def download_files(spec, dest):
    root = spec.get("root", "").strip("/")
    count = 0
    for inc in spec["include"]:
        for path, url in list_files(spec["repo"], spec["commit"], "/".join(p for p in (root, inc.strip("/")) if p)):
            rel = path[len(root) + 1:] if root else path
            target = os.path.join(dest, rel)
            os.makedirs(os.path.dirname(target), exist_ok=True)
            with open(target, "wb") as out:
                out.write(request(url))
            count += 1
    return count


def overlay_files(name):
    """(relative path, source path) of corpus/external/<name>/ in a stable order."""
    src = os.path.join(OVERLAY, name)
    for base, dirs, files in os.walk(src):
        dirs.sort()
        for f in sorted(files):
            path = os.path.join(base, f)
            yield os.path.relpath(path, src), path


def overlay_digest(name):
    """The digest apply_overlay would store, without copying (empty overlay: sha256 of nothing)."""
    digest = hashlib.sha256()
    for rel, path in overlay_files(name):
        with open(path, "rb") as source:
            digest.update(rel.encode() + b"\0" + source.read())
    return digest.hexdigest()[:16]


def apply_overlay(name, dest):
    """Copy corpus/external/<name>/ over the unpacked tree; returns a digest of it."""
    digest = hashlib.sha256()
    for rel, path in overlay_files(name):
        target = os.path.join(dest, rel)
        os.makedirs(os.path.dirname(target), exist_ok=True)
        shutil.copyfile(path, target)
        with open(path, "rb") as source:
            digest.update(rel.encode() + b"\0" + source.read())
    return digest.hexdigest()[:16]


def up_to_date(name, dest, spec):
    """A tree is installed when its receipt matches the pinned commit and the overlay."""
    receipt = os.path.join(dest, MARKER)
    if not os.path.isdir(dest) or not os.path.isfile(receipt):
        return False
    with open(receipt) as source:
        done = source.read().split()
    return done == [spec["commit"], overlay_digest(name)]


def stage_tree(name, spec, staging):
    """Download and unpack the pinned tree into `staging`; returns the file count."""
    if spec.get("fetch", "tarball") == "files":
        return download_files(spec, staging)
    data = request(f"https://codeload.github.com/{spec['repo']}/tar.gz/{spec['commit']}")
    if want := spec.get("sha256"):
        got = hashlib.sha256(data).hexdigest()
        if got != want:
            raise SystemExit(f"fetch-external: {name}: tarball sha256 {got}, manifest says {want}")
    return unpack_tarball(data, staging, spec.get("root"), spec.get("include", ()))


def fetch_repo(name, spec, force=False):
    """Install the pinned tree under DEST/name; staged, receipted, published atomically.

    Everything is built in a staging sibling and renamed into place under the
    store lock, so a partial download, a failed overlay or a failed rename
    leaves the previous complete tree in place (HARNESS-15). Reading an
    up-to-date install needs no write, so a worker may still use one.
    """
    dest = os.path.join(DEST, name)
    if not force and up_to_date(name, dest, spec):
        print(f"{name}: up to date ({spec['commit'][:12]})")
        return
    store = dependencies.require_owner()
    with dependencies.install_lock(store):
        if not force and up_to_date(name, dest, spec):
            print(f"{name}: up to date ({spec['commit'][:12]})")
            return
        dependencies.prune(dest)
        staging = dependencies.staging_dir(dest)
        try:
            count = stage_tree(name, spec, staging)
            if not count:
                raise SystemExit(f"fetch-external: {name}: nothing fetched (wrong root or include?)")
            overlay = apply_overlay(name, staging)
            with open(os.path.join(staging, MARKER), "w") as out:
                out.write(f"{spec['commit']} {overlay}\n")
            dependencies.publish(staging, dest)
            print(f"{name}: fetched {spec['repo']}@{spec['commit'][:12]} ({count} files)")
        finally:
            shutil.rmtree(staging, ignore_errors=True)


def overlay_uploaded(name, spec):
    """Copy corpus/external/<name>/ over an already-installed DEST/<name>; no download.

    The cloud launcher rsyncs third_party/ from the shared store (the main
    checkout's realpath), not from the branch under test, so a cfg or wrapper
    changed on the branch would silently not reach the instance (2026-10-09: a
    kraft lane ran the pre-fix, infinite-liveness cfg of its model). This is
    fetch_repo's overlay step, applied to that uploaded tree; the receipt keeps
    the tree's own commit and gets our overlay digest.

    Returns "applied <old>-><new>", "unchanged <digest>", or None (no tree to
    overlay: a verify run that uploaded only what its tests read).
    """
    dest = os.path.join(DEST, name)
    if not os.path.isdir(dest):
        return None
    commit, old = "", None
    receipt = os.path.join(dest, MARKER)
    if os.path.isfile(receipt):
        with open(receipt) as source:
            fields = source.read().split()
        commit = fields[0] if fields else ""
        old = fields[1] if len(fields) > 1 else None
    if commit and commit != spec["commit"]:
        print(f"{name}: WARNING the uploaded tree is {commit[:12]}, the manifest pins {spec['commit'][:12]}")
    digest = apply_overlay(name, dest)
    if digest == old:
        return f"unchanged {digest}"
    with open(receipt, "w") as out:
        out.write(f"{commit or spec['commit']} {digest}\n")
    return f"applied {old or '-'}->{digest}"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repos", default="", help="comma-separated repository names (default: all)")
    ap.add_argument("--force", action="store_true", help="download again even if at the pinned commit")
    ap.add_argument("--list", action="store_true", help="list the repositories and exit")
    ap.add_argument("--overlay-only", action="store_true",
                    help="copy corpus/external/<name>/ over an already-installed third_party/external/<name>/ "
                         "without downloading it: the uploaded tree comes from the shared store of the main checkout")
    a = ap.parse_args()
    manifest = load_manifest()
    # (The suites, e.g. tlcsuite's, are fetched by name or by their script, not by default.)
    repos = {**manifest.get("suites", {}), **manifest["repos"]}
    names = [n for n in a.repos.split(",") if n] or sorted(manifest["repos"])
    unknown = [n for n in names if n not in repos]
    if unknown:
        sys.exit(f"unknown repo(s): {', '.join(unknown)}; known: {', '.join(sorted(repos))}")
    if a.list:
        for n in names:
            r = repos[n]
            print(f"{n:24} {r['repo']}@{r['commit'][:12]} {r.get('license', '?')}")
        return
    if a.overlay_only:
        counts = {"applied": 0, "unchanged": 0, "absent": 0}
        for n in names:
            state = overlay_uploaded(n, repos[n])
            if state is None:
                counts["absent"] += 1
                print(f"{n}: no third_party/external/{n} to overlay, skipped")
                continue
            counts["applied" if state.startswith("applied") else "unchanged"] += 1
            print(f"{n}: overlay {state}")
        print(f"external overlays: {counts['applied']} applied, {counts['unchanged']} unchanged, "
              f"{counts['absent']} not installed")
        return
    for n in names:
        fetch_repo(n, repos[n], a.force)


if __name__ == "__main__":
    main()
