---
description: Thoth Release Workflow
allowed-tools: Bash, Read, Write, Edit, Glob, Grep
---

# Thoth Release Workflow

Execute a complete Thoth release using CalVer versioning and GitHub Actions CI/CD.

## Core Principles

A Thoth release is:

1. **CalVer-based**: Version is `YYYY.M.P` (current date in AEST)
2. **CI-built**: GitHub Actions builds, signs, and creates draft release
3. **Draft-first**: Review artifacts and edit release notes before publishing
4. **Auto-updatable**: Published releases trigger auto-updates for users

## Execution Steps

### 1. Determine New Version

**CRITICAL**: Always use current date in AEST (Australia/Brisbane, UTC+10)

```bash
# Get current date in AEST
TZ=Australia/Brisbane date +"%Y.%-m.0"
```

- **Patch bump** (fixes only): `2026.2.0` → `2026.2.1`
- **Month bump** (features, breaking): `2026.2.1` → `2026.3.0` (if new month)

Ask user to confirm the new version number.

### 2. Review Changes Since Last Release

```bash
# Find most recent tag
git describe --tags --abbrev=0 2>/dev/null || echo "No tags"

# Show changes since last tag (or all if no tags)
LAST_TAG=$(git describe --tags --abbrev=0 2>/dev/null || git rev-list --max-parents=0 HEAD)
git log $LAST_TAG..HEAD --oneline --no-merges

# Show full diff
git diff $LAST_TAG..HEAD
```

Analyse changes and summarise for release notes:

- Breaking changes (lead with these)
- New features
- Bug fixes
- Internal changes (optional)

### 3. Run Bump Script

```bash
./scripts/bump-version.sh <VERSION>
```

This updates:

- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `package.json`

### 4. Review Version Changes

```bash
git diff
```

Verify only version fields changed in the three files.

### 5. Commit and Tag

```bash
# Stage changes
git add -u

# Commit with standard message
git commit -m "chore(release): bump version to <VERSION>"

# Create tag
git tag v<VERSION>

# Push everything
git push origin main
git push origin v<VERSION>
```

### 6. Monitor CI Build

```bash
# Open actions page
open https://github.com/poodle64/thoth/actions
```

Tell user:

1. CI workflow will run (~10-15 minutes)
2. Watch for completion
3. Workflow creates draft release

### 7. Draft Release Instructions

Once CI completes, instruct user to:

1. Go to: https://github.com/poodle64/thoth/releases
2. Find draft release for the version
3. Verify artifacts present:
   - `Thoth_<VERSION>_aarch64.dmg`
   - `Thoth_<VERSION>_aarch64.app.tar.gz`
   - `Thoth_<VERSION>_aarch64.app.tar.gz.sig`
   - `latest.json`
4. Download and test the `.dmg` locally
5. Edit release notes with the summary from step 2
6. Publish release when ready

### 8. Post-Release

Remind user:

- Published release triggers auto-updates for users
- Monitor GitHub Issues for update problems
- Consider announcement (README, discussions, etc.)

## Failure Checks

Before pushing (step 5):

- [ ] Version determined using AEST date
- [ ] Changes reviewed and summarized
- [ ] Bump script ran successfully
- [ ] Only version fields changed (git diff clean)
- [ ] Commit message format correct
- [ ] Tag format is `v<VERSION>`

## Rollback

If critical issues discovered after publishing:

**Option 1: Hotfix Release (Recommended)**

1. Fix issue on `main`
2. Run this workflow again with next patch version
3. Publish immediately

**Option 2: Delete Release (NOT Recommended)**
⚠️ Breaks auto-updates for users who already upgraded
Only if no users upgraded AND issue is critical:

```bash
# Delete tag
git push --delete origin v<VERSION>
# Then delete release in GitHub UI
```

## Example Session

```bash
# 1. Determine version (16 Feb 2026 in AEST)
TZ=Australia/Brisbane date +"%Y.%-m.0"
# Output: 2026.2.0 (or .1, .2 if already released this month)

# 2. Review changes
LAST_TAG=$(git describe --tags --abbrev=0)
git log $LAST_TAG..HEAD --oneline

# 3. Bump version
./scripts/bump-version.sh 2026.2.3

# 4. Review
git diff

# 5. Commit and tag
git add -u
git commit -m "chore(release): bump version to 2026.2.3"
git tag v2026.2.3
git push origin main
git push origin v2026.2.3

# 6. Monitor
open https://github.com/poodle64/thoth/actions

# 7. Wait for CI, then review draft release
# 8. Publish when ready
```
