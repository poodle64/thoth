#!/usr/bin/env bash
#
# Regenerate the pnpmDeps hash in flake.nix.
#
# WHY THIS EXISTS
#
# `pnpmDeps` is a fixed-output derivation: Nix needs its content hash up front,
# so the hash has to be recorded in flake.nix. Any change to pnpm-lock.yaml
# changes that content and invalidates it. Cargo has no equivalent problem
# because `cargoLock.lockFile` derives everything from Cargo.lock's own
# per-crate checksums; nixpkgs has no pnpm equivalent, so this value is the one
# thing in the repo that a lockfile change cannot update by itself.
#
# That mattered because Renovate cannot run Nix. Every dependency PR it opened
# arrived with a stale hash and failed `Flake eval` until a human regenerated
# it by hand. This script is that step, so CI can do it instead of a person.
#
# THE WARM-STORE TRAP
#
# Do not "verify" a hash by running `nix build` on a machine that has built
# this before. A fixed-output derivation's store path is addressed by
# hash + name, so if the path for the OLD hash is already present, Nix skips
# the fetch entirely and reports success against a stale hash. CI, with a cold
# store, then fails. This script always goes through a deliberately wrong hash
# first, which forces a real fetch, because that path cannot already exist.
#
# Usage:
#   scripts/update-pnpm-deps-hash.sh          # rewrite flake.nix if needed
#   scripts/update-pnpm-deps-hash.sh --check  # exit 1 if stale, change nothing
#
# Exit codes:
#   0  hash already correct, or updated (without --check)
#   1  hash is stale (--check only)
#   2  something went wrong

set -euo pipefail

# Prefer the enclosing git worktree over the script's own location: CI runs a
# copy of this script extracted from the base branch (see nix-check.yaml), so
# the path it was invoked from is not necessarily inside the repo.
if REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)"; then
  :
else
  REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fi
FLAKE="${REPO_ROOT}/flake.nix"

# lib.fakeHash. Any value that cannot be the real one works; this is the
# conventional one and is recognisable in a diff if the script dies midway.
FAKE_HASH="sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

CHECK_ONLY=0
if [ "${1:-}" = "--check" ]; then
  CHECK_ONLY=1
elif [ -n "${1:-}" ]; then
  echo "unknown argument: $1" >&2
  echo "usage: $0 [--check]" >&2
  exit 2
fi

if ! command -v nix >/dev/null 2>&1; then
  echo "error: nix is not on PATH" >&2
  exit 2
fi

# Restrict every edit to the pnpmDeps block. flake.nix carries other `hash =`
# lines (the fluidaudio git dep, for one) and a repo-wide substitution would
# silently corrupt them.
readonly BLOCK_START='pnpmDeps = pkgs.fetchPnpmDeps {'

current_hash() {
  sed -n "/${BLOCK_START}/,/};/ s|.*hash = \"\\(sha256-[^\"]*\\)\";.*|\\1|p" "$FLAKE"
}

set_hash() {
  sed -i "/${BLOCK_START}/,/};/ s|hash = \"sha256-[^\"]*\";|hash = \"$1\";|" "$FLAKE"
}

ORIGINAL="$(current_hash)"
if [ -z "$ORIGINAL" ]; then
  echo "error: could not find the pnpmDeps hash in $FLAKE" >&2
  echo "       (looked for a hash inside the '${BLOCK_START}' block)" >&2
  exit 2
fi

# Any early exit from here on must put flake.nix back as we found it, or a
# failed run leaves the tree with a fake hash committed.
restore() {
  set_hash "$ORIGINAL"
}
trap restore EXIT

echo "Current pnpmDeps hash: $ORIGINAL"
echo "Forcing a real fetch to compute the true hash..."

set_hash "$FAKE_HASH"

# The build is expected to fail; the hash mismatch is the payload.
build_log="$(nix build "${REPO_ROOT}#thoth.pnpmDeps" --no-link 2>&1 || true)"

ACTUAL="$(printf '%s\n' "$build_log" \
  | sed -n 's|.*got: *\(sha256-[A-Za-z0-9+/=]*\).*|\1|p' \
  | head -1)"

if [ -z "$ACTUAL" ]; then
  echo "error: could not read the computed hash from the build output." >&2
  echo "       flake.nix has been restored to $ORIGINAL." >&2
  echo "--- build output ---" >&2
  printf '%s\n' "$build_log" >&2
  exit 2
fi

if [ "$ACTUAL" = "$ORIGINAL" ]; then
  # restore() has already put the original back, which is the correct value.
  echo "Hash is already correct: $ORIGINAL"
  exit 0
fi

echo "Hash is stale."
echo "  recorded: $ORIGINAL"
echo "  actual:   $ACTUAL"

if [ "$CHECK_ONLY" -eq 1 ]; then
  echo "Run scripts/update-pnpm-deps-hash.sh to fix it." >&2
  exit 1
fi

# Write the real hash and stop the trap from reverting it.
trap - EXIT
set_hash "$ACTUAL"
echo "Updated $FLAKE to $ACTUAL"
