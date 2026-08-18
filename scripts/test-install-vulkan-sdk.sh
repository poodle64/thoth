#!/usr/bin/env bash
set -euo pipefail

# Guards for scripts/install-vulkan-sdk.sh.
#
# The install used to be inlined, identically, in both ci.yaml and
# release.yaml. Two copies of the same apt incantation is exactly the drift the
# "Single source of truth" rule in .claude/CLAUDE.md exists to prevent, so
# these tests fail the build if anyone re-inlines it or stops calling the
# shared script. Run:
#
#   ./scripts/test-install-vulkan-sdk.sh

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/install-vulkan-sdk.sh"
WORKFLOWS=(
    "$REPO_ROOT/.github/workflows/ci.yaml"
    "$REPO_ROOT/.github/workflows/release.yaml"
)

pass=0
fail=0

check() {
    local name="$1" result="$2"
    if [ "$result" = "ok" ]; then
        echo "  PASS  $name"
        pass=$((pass + 1))
    else
        echo "  FAIL  $name"
        fail=$((fail + 1))
    fi
}

echo "install-vulkan-sdk.sh guards"

# The script must exist and be executable, or the workflows' `run:` line fails
# with a bare "Permission denied" that says nothing about why.
if [ -x "$SCRIPT" ]; then check "script exists and is executable" ok; else check "script exists and is executable" no; fi

# It must be valid bash. Catches a syntax error here rather than 30 minutes
# into a release build.
if bash -n "$SCRIPT" 2> /dev/null; then check "script parses" ok; else check "script parses" no; fi

for workflow in "${WORKFLOWS[@]}"; do
    name="$(basename "$workflow")"

    # Every workflow that needs the SDK must call the shared script. Anchored on
    # the `run:` line, not a bare filename match — the surrounding comments also
    # name the script, so a loose grep would pass even with the call deleted.
    if grep -qE '^\s*run: \./scripts/install-vulkan-sdk\.sh\s*$' "$workflow"; then
        check "$name calls the shared script" ok
    else
        check "$name calls the shared script" no
    fi

    # ...and must not have re-inlined the commands it replaced. Matching on the
    # LunarG host catches a copy-paste regression regardless of how the lines
    # are wrapped or which flags were used.
    if grep -q 'packages\.lunarg\.com' "$workflow"; then
        check "$name does not inline the LunarG install" no
    else
        check "$name does not inline the LunarG install" ok
    fi

    # A network step with no ceiling is what let a stalled fetch run 3.5 hours
    # on 2026-08-18; the job-level timeout is the backstop for everything else.
    if grep -q '^    timeout-minutes:' "$workflow"; then
        check "$name sets a job-level timeout" ok
    else
        check "$name sets a job-level timeout" no
    fi
done

echo
echo "passed: $pass  failed: $fail"
[ "$fail" -eq 0 ]
