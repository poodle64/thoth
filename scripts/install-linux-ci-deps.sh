#!/usr/bin/env bash
set -euo pipefail

# Prepare an Ubuntu 22.04 (jammy) CI runner to build Thoth: the LunarG Vulkan
# SDK plus the GUI/audio development libraries Tauri needs.
#
# This is the single definition of that setup (see the "Single source of truth"
# rule in .claude/CLAUDE.md). Both .github/workflows/ci.yaml and
# .github/workflows/release.yaml call it.
#
# Why the Vulkan SDK and not Ubuntu's own packages: whisper.cpp's GGML Vulkan
# backend compiles its compute shaders at build time, so its CMake does
# find_package(Vulkan COMPONENTS glslc REQUIRED) and
# find_package(SPIRV-Headers REQUIRED). The glslc binary is NOT packaged in
# jammy at all — it only appears from noble onward — so the LunarG apt repo is
# the supported way to get it on a jammy runner. vulkan-sdk supplies glslc, the
# loader/headers, and the SPIRV headers, replacing libvulkan-dev/spirv-headers.
# At runtime users need only libvulkan.so.1 plus a GPU Vulkan driver.
#
# ---------------------------------------------------------------------------
# Why this is written so defensively
#
# Three separate CI failures in two days, all mirror-related, none of them a
# fault in the build:
#
#   1. A LunarG fetch ran 3.5 hours. wget defaults to 20 tries against a
#      900-second read timeout, i.e. up to five hours against a dead mirror.
#      Fixed by bounding wget and adding a job-level ceiling.
#   2. A 10-minute step ceiling killed a healthy `apt-get install` that was
#      merely slow — 101 packages downloaded and still progressing.
#   3. `apt-get update` sat for 24.5 minutes emitting nothing, against a
#      failing azure.archive.ubuntu.com, and blew a 25-minute ceiling.
#
# Case 3 is the important one: apt's Acquire::*::Timeout options were already
# set and did not help, because they bound individual socket operations, not
# the invocation. A mirror that accepts a connection and then stalls between
# files keeps apt waiting without ever tripping them.
#
# So every network operation here is bounded by wall clock and retried:
# a stalled mirror becomes a fast, retryable failure instead of a hang, while
# a merely slow mirror is still allowed to finish. A healthy run takes well
# under a minute and none of this machinery engages.
# ---------------------------------------------------------------------------
#
# Usage: ./scripts/install-linux-ci-deps.sh

KEY_URL="https://packages.lunarg.com/lunarg-signing-key-pub.asc"
LIST_URL="http://packages.lunarg.com/vulkan/lunarg-vulkan-jammy.list"

# Bounds each socket operation. Necessary but NOT sufficient on its own — see
# case 3 above; apt_run's wall-clock bound is what actually catches a stall.
APT_OPTS=(
    -o Acquire::http::Timeout=30
    -o Acquire::https::Timeout=30
    -o Acquire::Retries=3
)

# Wall-clock bound per apt attempt. Generous: a healthy update or install
# finishes in under a minute, so this only fires on a genuine stall.
APT_ATTEMPT_TIMEOUT=300

PACKAGES=(
    libgtk-3-dev
    libwebkit2gtk-4.1-dev
    libappindicator3-dev
    librsvg2-dev
    patchelf
    libasound2-dev
    # Provides desktop-file-validate, used by ci.yaml's desktop entry check.
    # This was previously missing from release.yaml's copy of the list.
    desktop-file-utils
)

# Run an apt operation under a wall-clock bound, retrying on failure.
apt_run() {
    local attempt
    for attempt in 1 2 3; do
        if sudo timeout "$APT_ATTEMPT_TIMEOUT" apt-get "${APT_OPTS[@]}" "$@"; then
            return 0
        fi
        echo "::warning::apt-get $1 failed or exceeded ${APT_ATTEMPT_TIMEOUT}s (attempt ${attempt}/3); retrying in 10s"
        # A timeout kills apt mid-transaction, which can leave dpkg needing to
        # finish configuring. Harmless when there is nothing pending.
        sudo dpkg --configure -a > /dev/null 2>&1 || true
        sleep 10
    done
    echo "::error::apt-get $1 failed after 3 attempts"
    return 1
}

# Fetch a URL to a file, failing fast and retrying a bounded number of times.
#
# The empty-file check is not paranoia. The original form of this code piped
# wget straight into `sudo tee`, which reports the exit status of tee, not
# wget — so a failed download wrote an EMPTY signing key, and the job then died
# much later at `apt-get update` with a confusing GPG error rather than at the
# point of failure.
fetch_to() {
    local dest="$1" url="$2" attempt

    for attempt in 1 2 3; do
        if wget --timeout=30 --tries=2 -qO "$dest" "$url" && [ -s "$dest" ]; then
            return 0
        fi
        echo "::warning::fetch of ${url} failed or was empty (attempt ${attempt}/3); retrying in 5s"
        sleep 5
    done

    echo "::error::could not fetch ${url} after 3 attempts"
    return 1
}

echo "::group::Add the LunarG apt repository"
fetch_to /tmp/lunarg-signing-key.asc "$KEY_URL"
fetch_to /tmp/lunarg-vulkan-jammy.list "$LIST_URL"
sudo install -m 0644 /tmp/lunarg-signing-key.asc /etc/apt/trusted.gpg.d/lunarg.asc
sudo install -m 0644 /tmp/lunarg-vulkan-jammy.list /etc/apt/sources.list.d/lunarg-vulkan-jammy.list
echo "::endgroup::"

echo "::group::apt-get update"
apt_run update
echo "::endgroup::"

echo "::group::Install the Vulkan SDK"
apt_run install -y vulkan-sdk
echo "::endgroup::"

echo "::group::Install Tauri build dependencies"
apt_run install -y "${PACKAGES[@]}"
echo "::endgroup::"

# Fail here, at the point of the actual problem, rather than several minutes
# later inside a CMake configure step with a less obvious error.
if ! command -v glslc > /dev/null; then
    echo "::error::vulkan-sdk installed but glslc is not on PATH; the whisper.cpp Vulkan build will fail"
    exit 1
fi

echo "Linux CI dependencies ready: $(glslc --version | head -1)"
