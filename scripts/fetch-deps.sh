#!/usr/bin/env bash
# Fetch test-only dependencies into third_party/ (mirrors tlaplus/Examples CI deps).
#
# third_party/ is shared with every linked worktree, which reaches it through a
# symlink. Only the checkout that owns the tree may install into it, the whole
# fetch runs under the one installer lock of scripts/dependencies.py, and every
# artifact is staged next to its target and renamed into place, so a failed or
# interrupted fetch never leaves a partial final file (HARNESS-15).
set -euo pipefail
self="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
root="$(cd "$(dirname "$self")/.." && pwd)"
cd "$root"
if [ -z "${FETCH_DEPS_LOCKED:-}" ]; then
  exec python3 scripts/dependencies.py run -- env FETCH_DEPS_LOCKED=1 bash "$self"
fi

deps() { python3 "$root/scripts/dependencies.py" "$@"; }

mkdir -p third_party
cd third_party

# download URL FILE: stage in the target directory, require a non-empty body,
# then publish with one rename; FILE is untouched unless the body is complete.
download() {
  url="$1"; file="$2"
  if [ -e "$file" ]; then return 0; fi
  mkdir -p "$(dirname "$file")"
  tmp="$(mktemp "$(dirname "$file")/.$(basename "$file").XXXXXX")"
  if ! curl -sSfL -o "$tmp" "$url" || [ ! -s "$tmp" ]; then
    rm -f "$tmp"
    echo "fetch-deps: failed or empty download: $url" >&2
    return 1
  fi
  mv -f "$tmp" "$file"
}

# tla2tools.jar is the TLC oracle baseline: only the exact bytes declared in
# scripts/dependencies.py (TLA2TOOLS_PIN) may be installed or kept (DOC-09). The
# v1.8.0 release asset has been re-uploaded with another revision, so a download
# is verified before it is published and an existing jar is verified instead of
# trusted; a mismatch is refused, never silently used.
tla2tools() {
  file=tla2tools.jar
  if [ -e "$file" ]; then
    if ! deps verify-jar "$file"; then
      echo "fetch-deps: refusing an existing $file that is not the declared baseline;" >&2
      echo "  remove it to download the pinned bytes, or restore the pinned jar:" >&2
      echo "  scripts/dependencies.py pin" >&2
      return 1
    fi
    return 0
  fi
  url="$(deps pin url)"
  tmp="$(mktemp "$(dirname "$file")/.$(basename "$file").XXXXXX")"
  if ! curl -sSfL -o "$tmp" "$url" || [ ! -s "$tmp" ]; then
    rm -f "$tmp"
    echo "fetch-deps: failed or empty download: $url" >&2
    return 1
  fi
  if ! deps verify-jar "$tmp"; then
    rm -f "$tmp"
    echo "fetch-deps: refusing the bytes served by $url for $file" >&2
    return 1
  fi
  mv -f "$tmp" "$file"
}
tla2tools

if [ ! -d Examples ]; then
  clone="$(mktemp -d .Examples.XXXXXX)"
  git clone -q --depth 1 https://github.com/tlaplus/Examples.git "$clone"
  mv "$clone" Examples
fi
# Examples keeps CCF, azure-cosmos-tla, BlockingQueue and microwave in submodules
# (idempotent: also fills a clone made without them).
git -C Examples submodule update -q --init --depth 1

# The Examples models whose manifest runtime is above 700 s (the huge tier of
# scripts/corpus.py: `corpus.py list --tier huge`) are for the cloud runs only; the
# external corpora (corpus/external.json) are fetched by scripts/fetch-external.py.
if [ ! -d community ]; then
  staged="$(mktemp -d .community.XXXXXX)"
  download https://github.com/tlaplus/CommunityModules/releases/latest/download/CommunityModules-deps.jar "$staged/modules.jar"
  (cd "$staged" && unzip -q -o modules.jar '*.tla' -d .)
  mv "$staged" community
fi

if [ ! -d tlapm ]; then
  staged="$(mktemp -d .tlapm.XXXXXX)"
  download https://github.com/tlaplus/tlapm/archive/main.tar.gz "$staged/main.tar.gz"
  tar -xzf "$staged/main.tar.gz" -C "$staged"
  mv "$staged/tlapm-main" tlapm
  rm -rf "$staged"
fi

for m in Apalache Variants Option; do
  download "https://raw.githubusercontent.com/apalache-mc/apalache/main/src/tla/$m.tla" "apalache/$m.tla"
done
echo ok
