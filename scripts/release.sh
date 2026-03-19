#!/usr/bin/env bash
# release.sh — Build and publish a Thoth release
#
# Usage:
#   ./scripts/release.sh <version> [--notes "Release notes"]
#
# Prerequisites:
#   - TAURI_SIGNING_PRIVATE_KEY_PATH  path to signing key
#   - TAURI_SIGNING_PRIVATE_KEY_PASSWORD  key password
#   - gh CLI authenticated
#   - Rust + Node installed
#
# Example:
#   TAURI_SIGNING_PRIVATE_KEY_PATH=~/.tauri/key.key \
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD="secret" \
#   ./scripts/release.sh 2026.3.0 --notes "New features"

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FIXES_SCRIPT="$SCRIPT_DIR/fix-permissions.sh"

RED='\033[0;31m'; GREEN='\033[0;32m'; CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'
ok()   { echo -e "  ${GREEN}✔${NC} $*"; }
step() { echo -e "\n${BOLD}${CYAN}▶${NC} $*"; }
fail() { echo -e "  ${RED}✘${NC} $*"; exit 1; }

# ── Args ──────────────────────────────────────────────────────────────────────
VERSION="${1:-}"
NOTES=""
shift || true
while [[ $# -gt 0 ]]; do
  case "$1" in
    --notes) NOTES="$2"; shift 2 ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

[[ -z "$VERSION" ]] && { echo "Usage: $0 <version> [--notes \"...\"]"; exit 1; }

# ── Bump version ──────────────────────────────────────────────────────────────
step "Bumping version to $VERSION"
"$SCRIPT_DIR/bump-version.sh" "$VERSION"
ok "Version bumped"

# ── Build ─────────────────────────────────────────────────────────────────────
step "Building (aarch64-apple-darwin)"
cd "$REPO_ROOT"
npm run tauri build -- --target aarch64-apple-darwin
ok "Build complete"

# ── Locate artifacts ─────────────────────────────────────────────────────────
DMG="$REPO_ROOT/src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Thoth_${VERSION}_aarch64.dmg"
TAR="$REPO_ROOT/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Thoth.app.tar.gz"
TAR_DEST="$REPO_ROOT/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Thoth_${VERSION}_aarch64.app.tar.gz"

[[ -f "$DMG" ]] || fail "DMG not found: $DMG"
[[ -f "$TAR" ]] || fail "tar.gz not found: $TAR"
cp "$TAR" "$TAR_DEST"

# ── Sign ──────────────────────────────────────────────────────────────────────
step "Signing app bundle for updater"
SIG=$(npm run tauri -- signer sign \
  -f "$TAURI_SIGNING_PRIVATE_KEY_PATH" \
  -p "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" \
  "$TAR_DEST" 2>&1 | awk '/Public signature:/{getline; print}')
[[ -z "$SIG" ]] && fail "Signing failed — check key path and password"
ok "Signed"

# ── latest.json ───────────────────────────────────────────────────────────────
step "Generating latest.json"
LATEST_JSON="$REPO_ROOT/src-tauri/target/latest.json"
python3 - <<PYEOF
import json, sys
sig = """$SIG"""
d = {
  "version": "$VERSION",
  "notes": """$NOTES""",
  "pub_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "platforms": {
    "darwin-aarch64": {
      "signature": sig.strip(),
      "url": "https://github.com/sk8ersquare/thoth/releases/download/v$VERSION/Thoth_${VERSION}_aarch64.app.tar.gz"
    }
  }
}
with open("$LATEST_JSON", "w") as f:
    json.dump(d, f, indent=2)
print("  Written:", "$LATEST_JSON")
PYEOF
ok "latest.json ready"

# ── Commit & tag ──────────────────────────────────────────────────────────────
step "Committing and tagging v$VERSION"
git add -A
git commit -m "release: v$VERSION"
git tag "v$VERSION"
git push origin main "v$VERSION"
ok "Pushed"

# ── GitHub release ────────────────────────────────────────────────────────────
step "Creating GitHub release v$VERSION"
gh release create "v$VERSION" \
  --title "v$VERSION" \
  --notes "${NOTES:-Release v$VERSION}" \
  "$DMG" \
  "$TAR_DEST" \
  "$LATEST_JSON" \
  "$FIXES_SCRIPT" \
  --repo sk8ersquare/thoth
ok "Release published: https://github.com/sk8ersquare/thoth/releases/tag/v$VERSION"
