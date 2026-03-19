#!/usr/bin/env bash
# fix-permissions.sh — Thoth macOS Permission Reset
# Run this after installing a new version of Thoth if it won't start,
# hotkeys stop working, or microphone/accessibility prompts never appear.
#
# Usage:  bash fix-permissions.sh
#   or:   bash fix-permissions.sh --no-relaunch
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

APP_PATH="/Applications/Thoth.app"
BUNDLE_ID="com.poodle64.thoth"
NO_RELAUNCH="${1:-}"

# ── Colours ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'

header()  { echo -e "\n${BOLD}${CYAN}▶ $*${NC}"; }
ok()      { echo -e "  ${GREEN}✔${NC} $*"; }
warn()    { echo -e "  ${YELLOW}⚠${NC}  $*"; }
fail()    { echo -e "  ${RED}✘${NC} $*"; }
step()    { echo -e "  ${BOLD}→${NC} $*"; }

echo ""
echo -e "${BOLD}Thoth — macOS Permission Reset${NC}"
echo "────────────────────────────────"

# ── 1. Verify app exists ──────────────────────────────────────────────────────
header "Step 1 — Locate Thoth"
if [ -d "$APP_PATH" ]; then
  VERSION=$(defaults read "$APP_PATH/Contents/Info.plist" CFBundleShortVersionString 2>/dev/null || echo "unknown")
  ok "Found Thoth v$VERSION at $APP_PATH"
else
  fail "Thoth not found at $APP_PATH"
  echo ""
  echo "  Install Thoth first, then re-run this script."
  exit 1
fi

# ── 2. Quit Thoth if running ──────────────────────────────────────────────────
header "Step 2 — Quit Thoth"
if pgrep -x "thoth" > /dev/null 2>&1; then
  step "Quitting Thoth..."
  osascript -e 'tell application "Thoth" to quit' 2>/dev/null || pkill -x thoth 2>/dev/null || true
  sleep 1
  ok "Thoth stopped"
else
  ok "Thoth is not running"
fi

# ── 3. Remove quarantine flag ─────────────────────────────────────────────────
header "Step 3 — Remove macOS quarantine flag"
step "Running: xattr -dr com.apple.quarantine $APP_PATH"
if xattr -dr com.apple.quarantine "$APP_PATH" 2>/dev/null; then
  ok "Quarantine flag removed (or was already clear)"
else
  warn "Could not remove quarantine flag — may already be clear"
fi

# ── 4. Reset TCC permissions ──────────────────────────────────────────────────
header "Step 4 — Reset system permissions"
echo "  This clears the old grants so macOS will re-prompt for each one."
echo ""

reset_tcc() {
  local SERVICE="$1"; local LABEL="$2"
  step "Resetting $LABEL..."
  if tccutil reset "$SERVICE" "$BUNDLE_ID" > /dev/null 2>&1; then
    ok "$LABEL reset — Thoth will re-request this on next launch"
  else
    warn "Could not reset $LABEL (may already be unset)"
  fi
}

reset_tcc "ListenEvent"    "Input Monitoring (global hotkeys)"
reset_tcc "Accessibility"  "Accessibility (paste / text insertion)"
reset_tcc "Microphone"     "Microphone"

# ── 5. Open System Settings for each service ─────────────────────────────────
header "Step 5 — Open Privacy & Security settings"
echo "  macOS will re-prompt when Thoth first needs each permission."
echo "  You can also grant them manually in System Settings:"
echo ""
step "Opening Privacy & Security → Accessibility..."
open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
sleep 1
step "Opening Privacy & Security → Input Monitoring..."
open "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"

# ── 6. Relaunch ───────────────────────────────────────────────────────────────
if [ "$NO_RELAUNCH" != "--no-relaunch" ]; then
  header "Step 6 — Relaunch Thoth"
  step "Starting Thoth..."
  sleep 1
  open -a "$APP_PATH"
  echo ""
  ok "Thoth launched"
  echo ""
  echo -e "  ${BOLD}What happens next:${NC}"
  echo "  • Thoth will ask for Microphone access on first recording"
  echo "  • It will ask for Accessibility on first paste attempt"
  echo "  • Input Monitoring prompt appears when a hotkey is registered"
  echo "  • In each dialog: click OK / Allow"
  echo ""
  echo "  If the prompts don't appear, open System Settings and grant"
  echo "  permissions manually for the services listed above."
fi

echo ""
echo -e "${GREEN}${BOLD}Done!${NC}"
echo "────────────────────────────────"
echo ""
