#!/usr/bin/env bash
set -euo pipefail

# Install the GUI and audio development libraries Tauri needs to compile on an
# Ubuntu 22.04 (jammy) CI runner.
#
# The Vulkan toolchain is NOT installed here — scripts/install-vulkan-sdk.sh
# owns that, and must run first (it also performs the `apt-get update` this
# script relies on).
#
# This is the single definition of the package list (see the "Single source of
# truth" rule in .claude/CLAUDE.md). Both .github/workflows/ci.yaml and
# .github/workflows/release.yaml call it. They previously inlined two lists
# that had already drifted: only ci.yaml installed desktop-file-utils. The
# lists are now one, which costs release.yaml one small extra package and
# removes the drift.
#
# On timeouts: this step pulls ~100 packages including the WebKitGTK stack,
# which is large. On 2026-08-18 a slow Azure mirror took it past a 10-minute
# step ceiling and failed a build that would otherwise have succeeded — so the
# apt options below bound each connection rather than the wall clock. A mirror
# that has stopped responding now errors quickly, while a mirror that is merely
# slow is allowed to finish. The caller's timeout-minutes is the backstop for a
# true hang, and is deliberately generous for that reason.
#
# Usage: ./scripts/install-linux-build-deps.sh

# Bound each connection so a stalled mirror fails fast instead of hanging, but
# do not cap total transfer time — a slow mirror should still complete.
APT_OPTS=(
    -o Acquire::http::Timeout=30
    -o Acquire::https::Timeout=30
    -o Acquire::Retries=3
)

PACKAGES=(
    libgtk-3-dev
    libwebkit2gtk-4.1-dev
    libappindicator3-dev
    librsvg2-dev
    patchelf
    libasound2-dev
    # Provides desktop-file-validate, used by ci.yaml's desktop entry check.
    desktop-file-utils
)

sudo apt-get "${APT_OPTS[@]}" install -y "${PACKAGES[@]}"
