# Auto-Update End-to-End Testing Guide

This document provides step-by-step instructions for validating the complete auto-update pipeline.

## Prerequisites

Before testing, you must have:

1. **At least one published release** - The baseline version to update FROM
2. **A newer version ready** - The target version to update TO
3. **Clean test environment** - Fresh macOS install or VM recommended

## Test Scenario: First Release Validation

**Goal**: Validate update mechanism from v2026.2.3 → v2026.2.4 (or later)

### Step 1: Publish Baseline Release

1. Create and publish v2026.2.3 using `/git-release`:

   ```bash
   /git-release
   # Follow prompts, publish the draft release
   ```

2. Download and install the `.dmg` locally
3. Launch and verify basic functionality works
4. **Keep this version installed** - this becomes your "old version"

### Step 2: Create Update Release

1. Make a trivial change (e.g., update version only or add a comment)
2. Run `/git-release` again to create v2026.2.4:

   ```bash
   /git-release
   # Bump to next patch version
   ```

3. Publish the draft release
4. **Wait 2-3 minutes** for GitHub CDN to propagate `latest.json`

### Step 3: Test Update Flow

With v2026.2.3 still running:

1. **Launch the app** (if not already running)
2. Wait 2 seconds for auto-check to complete
3. **Verify update banner appears**:
   - Banner shows "Update Available"
   - Version shown: v2026.2.4
   - "Update Now" and "Later" buttons visible

4. **Click "Update Now"**:
   - Banner changes to "Downloading Update..."
   - Progress bar animates (0% → 100%)
   - Banner changes to "Update Ready"
   - Button changes to "Restart"

5. **Click "Restart"**:
   - App closes
   - App relaunches automatically
   - New version loads

6. **Verify post-update state**:
   - App shows v2026.2.4 in About dialogue or version display
   - Settings → Overview shows "Up to date" status
   - All settings preserved (check config.json untouched)
   - Transcription still works (model files still present)
   - History window shows previous transcriptions

### Step 4: Edge Case Testing

#### Test 4.1: Offline Handling

1. Disconnect from network (turn off Wi-Fi)
2. Quit and relaunch app
3. **Expected**: No crash, update check silently fails
4. Open Settings → Overview
5. Click "Check for Updates" manually
6. **Expected**: Error state, graceful message like "Failed to check for updates"
7. Reconnect network

#### Test 4.2: Dismiss and Reappear

1. Ensure update is available (or publish another version)
2. When banner appears, click "Later"
3. **Expected**: Banner dismisses
4. Quit and relaunch app
5. **Expected**: Banner reappears after 2s delay

#### Test 4.3: Auto-Check Disabled

1. Open Settings → Overview
2. Uncheck "Automatically check for updates on launch"
3. Quit and relaunch app
4. **Expected**: No banner appears on launch
5. Open Settings → Overview
6. Click "Check for Updates" button
7. **Expected**: Manual check works, banner appears if update available

#### Test 4.4: Downgrade Prevention

1. Install a NEWER version (e.g., v2026.2.5)
2. Publish an OLDER version as "latest" (e.g., v2026.2.4)
   - Edit `latest.json` manually if needed, or delete newer release
3. Launch the newer installed version (v2026.2.5)
4. **Expected**: No update offered (updater should detect current ≥ latest)

#### Test 4.5: Partial Download Recovery

1. Start update download
2. Mid-download, disconnect network (turn off Wi-Fi)
3. **Expected**: Download fails, error state shown
4. Reconnect network
5. Click "Update Now" again
6. **Expected**: Download restarts from beginning

#### Test 4.6: Insufficient Disk Space

1. Fill disk until <50MB free (or create large files to simulate)
2. Attempt to download update (~30-40MB)
3. **Expected**: Download fails gracefully with disk space error
4. Free up space and retry

#### Test 4.7: App Quit During Download

1. Start update download
2. Mid-download (e.g., 50%), quit the app (Cmd+Q)
3. Relaunch the app
4. **Expected**: Update state resets, banner reappears
5. Click "Update Now" again
6. **Expected**: Download starts fresh (no partial resume)

## Acceptance Criteria Checklist

- [ ] Auto-check on launch detects update (when enabled)
- [ ] Update banner displays correct version and UI states
- [ ] Download progress bar updates smoothly
- [ ] Relaunch after install succeeds
- [ ] Post-update version is correct
- [ ] Config preserved (`~/.thoth/config.json` unchanged)
- [ ] History preserved (database intact)
- [ ] Models preserved (`~/.thoth/Models/` untouched)
- [ ] Offline handling graceful (no crash, error message)
- [ ] Dismiss and relaunch shows banner again
- [ ] Auto-check can be disabled
- [ ] Manual check still works when auto-check disabled
- [ ] Downgrade not offered
- [ ] Partial download recovers gracefully
- [ ] Insufficient disk space handled
- [ ] App quit during download resets state

## Troubleshooting

### Banner never appears

**Possible causes:**

- `latest.json` not published (check release artefacts)
- Endpoint URL wrong in `tauri.conf.json`
- GitHub CDN not yet updated (wait 5 minutes)
- Auto-check disabled in settings

**Debug steps:**

```bash
# Verify latest.json exists
curl -I https://github.com/poodle64/thoth/releases/latest/download/latest.json
```

**Check app logs:**

1. Open Console.app
2. Filter by "Thoth" in search bar
3. Look for these log patterns:
   - `[App] Checking for updates...` (check initiated)
   - `Update check failed:` (error state)
   - Tauri updater plugin logs (signature validation, download)

### Download fails

**Possible causes:**

- Signature verification failed (pubkey mismatch)
- Download URL incorrect in `latest.json`
- Network connectivity lost mid-download

**Debug steps:**

- Check Console.app for Tauri updater errors
- Verify `.sig` file exists in release
- Test download URL manually:
  ```bash
  curl -I https://github.com/poodle64/thoth/releases/download/vX.Y.Z/Thoth_X.Y.Z_aarch64.app.tar.gz
  ```

### Relaunch doesn't happen

**Possible causes:**

- `tauri-plugin-process` not initialised
- Permissions issue preventing relaunch

**Debug steps:**

- Check Console.app for process relaunch errors
- Verify capabilities include `"process:default"`

### Settings lost after update

**This is a BUG** - config should be preserved. Investigate:

- Check `~/.thoth/config.json` before and after update
- Verify Tauri config doesn't relocate app data
- File a bug report with reproduction steps

## Continuous Testing

For ongoing releases:

1. **Before each release**: Test update from current published version
2. **After each release**: Verify update notification appears for users
3. **Monitor**: Check GitHub Issues for update-related problems

## Automated Testing (Future)

Consider automating parts of this workflow:

- **Unit tests**: Mock updater check response
- **Integration tests**: Test update state machine transitions
- **E2E tests**: Tauri WebDriver for UI automation (requires published releases)

For now, manual testing is required due to the dependency on real GitHub releases and macOS app installation.
