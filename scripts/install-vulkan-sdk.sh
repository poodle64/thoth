#!/usr/bin/env bash
set -euo pipefail

# Install the LunarG Vulkan SDK on an Ubuntu 22.04 (jammy) CI runner.
#
# Why the SDK and not Ubuntu's own packages: whisper.cpp's GGML Vulkan backend
# compiles its compute shaders at build time, so its CMake does
# find_package(Vulkan COMPONENTS glslc REQUIRED) and
# find_package(SPIRV-Headers REQUIRED). The glslc binary is NOT packaged in
# jammy at all — it only appears from noble onward — so the LunarG apt repo is
# the supported way to get it on a jammy runner. vulkan-sdk supplies glslc, the
# loader/headers, and the SPIRV headers, replacing libvulkan-dev/spirv-headers.
# At runtime users need only libvulkan.so.1 plus a GPU Vulkan driver.
#
# This is the single definition of that install (see the "Single source of
# truth" rule in .claude/CLAUDE.md). Both .github/workflows/ci.yaml and
# .github/workflows/release.yaml call it; neither inlines the commands.
#
# Why it is written defensively rather than as four plain commands: on
# 2026-08-18 a CI run sat in this step for 3.5 hours before anyone noticed.
# wget's defaults are 20 tries against a 900-second read timeout, i.e. up to
# five hours against an unresponsive mirror, and apt waits on a stalled mirror
# just as patiently. Every network call below is bounded so a flaky repo fails
# in seconds and retries deliberately, instead of silently eating the job's
# entire time budget. The callers add timeout-minutes as a second backstop.
#
# Usage: ./scripts/install-vulkan-sdk.sh

KEY_URL="https://packages.lunarg.com/lunarg-signing-key-pub.asc"
LIST_URL="http://packages.lunarg.com/vulkan/lunarg-vulkan-jammy.list"

# Bound apt the same way wget is bounded below; without these it blocks
# indefinitely on a mirror that accepts the connection then stops responding.
APT_OPTS=(
    -o Acquire::http::Timeout=30
    -o Acquire::https::Timeout=30
    -o Acquire::Retries=3
)

# Fetch a URL to a file, failing fast and retrying a bounded number of times.
#
# The empty-file check is not paranoia. The original form of this code piped
# wget straight into `sudo tee`, which reports the exit status of tee, not
# wget — so a failed download wrote an EMPTY signing key, and the job then
# died much later at `apt-get update` with a confusing GPG error rather than
# at the point of failure.
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

fetch_to /tmp/lunarg-signing-key.asc "$KEY_URL"
fetch_to /tmp/lunarg-vulkan-jammy.list "$LIST_URL"

sudo install -m 0644 /tmp/lunarg-signing-key.asc /etc/apt/trusted.gpg.d/lunarg.asc
sudo install -m 0644 /tmp/lunarg-vulkan-jammy.list /etc/apt/sources.list.d/lunarg-vulkan-jammy.list

sudo apt-get "${APT_OPTS[@]}" update
sudo apt-get "${APT_OPTS[@]}" install -y vulkan-sdk

# Fail here, at the point of the actual problem, rather than several minutes
# later inside a CMake configure step with a less obvious error.
if ! command -v glslc > /dev/null; then
    echo "::error::vulkan-sdk installed but glslc is not on PATH; the whisper.cpp Vulkan build will fail"
    exit 1
fi

echo "Vulkan SDK ready: $(glslc --version | head -1)"
