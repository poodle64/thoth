#!/usr/bin/env bash
set -euo pipefail

# Guards for the shared Linux CI setup scripts.
#
# Both installs used to be inlined in ci.yaml and release.yaml. Two copies of
# the same apt incantation is exactly the drift the "Single source of truth"
# rule in .claude/CLAUDE.md exists to prevent — and they HAD drifted: only
# ci.yaml installed desktop-file-utils. These tests fail the build if anyone
# re-inlines either install or stops calling the shared script. Run:
#
#   ./scripts/test-linux-build-scripts.sh

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPTS=(
    "$REPO_ROOT/scripts/install-linux-ci-deps.sh"
)
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

echo "Linux build script guards"

for script in "${SCRIPTS[@]}"; do
    name="$(basename "$script")"

    # Must exist and be executable, or the workflows' `run:` line fails with a
    # bare "Permission denied" that says nothing about why.
    if [ -x "$script" ]; then check "$name is executable" ok; else check "$name is executable" no; fi

    # Must be valid bash. Catches a syntax error here rather than 30 minutes
    # into a release build.
    if bash -n "$script" 2> /dev/null; then check "$name parses" ok; else check "$name parses" no; fi
done

for workflow in "${WORKFLOWS[@]}"; do
    name="$(basename "$workflow")"

    # Every workflow must call the shared script. Anchored on the `run:` line,
    # not a bare filename match — the surrounding comments also name the
    # script, so a loose grep would pass even with the call deleted.
    if grep -qE '^\s*run: \./scripts/install-linux-ci-deps\.sh\s*$' "$workflow"; then
        check "$name calls install-linux-ci-deps.sh" ok
    else
        check "$name calls install-linux-ci-deps.sh" no
    fi

    # The apt package list must live in the script, not back in the workflow.
    if grep -qE 'apt-get +install' "$workflow"; then
        check "$name does not inline an apt install" no
    else
        check "$name does not inline an apt install" ok
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

# The wall-clock bound on each apt attempt is what survives a stalled mirror;
# apt's own Acquire timeouts demonstrably do not (2026-08-19, 24.5 minutes of
# silence with them set). Losing this silently reintroduces that hang.
#
# shellcheck disable=SC2016  # matching the literal $APT_ATTEMPT_TIMEOUT in the source
if grep -qE 'timeout "\$APT_ATTEMPT_TIMEOUT" apt-get' "$REPO_ROOT/scripts/install-linux-ci-deps.sh"; then
    check "apt calls are wall-clock bounded" ok
else
    check "apt calls are wall-clock bounded" no
fi

echo
echo "passed: $pass  failed: $fail"
[ "$fail" -eq 0 ]
