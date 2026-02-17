# Release Process

This document describes the process for creating a new Thoth release.

## Versioning Scheme

Thoth uses **CalVer** (Calendar Versioning): `YYYY.M.P`

- `YYYY` - Four-digit year (e.g., 2026)
- `M` - Month without leading zero (e.g., 2 for February, 10 for October)
- `P` - Patch number, starts at 0 for each month (e.g., 0, 1, 2...)

Examples: `2026.2.0`, `2026.2.1`, `2026.10.0`

CalVer is SemVer-compatible for the Tauri updater plugin (major.minor.patch).

## Prerequisites

- All changes committed and pushed to `main` branch
- All tests passing
- CI workflow configured (`.github/workflows/release.yml`)
- Signing key configured as `TAURI_SIGNING_PRIVATE_KEY` secret
- Clean working directory (`git status` shows no uncommitted changes)

## Release Steps

### 1. Bump Version

Use the version bump script to update all three version files:

```bash
./scripts/bump-version.sh 2026.3.0
```

This updates:

- `src-tauri/Cargo.toml` - Rust package version
- `src-tauri/tauri.conf.json` - Tauri app version
- `package.json` - Node package version

**Manual alternative:**
Edit each file directly and search for `"version"` field.

### 2. Review Changes

```bash
git diff
```

Verify that only the version fields changed in all three files.

### 3. Commit Version Bump

```bash
git add -u
git commit -m "chore(release): bump version to 2026.3.0"
```

Use the exact format: `chore(release): bump version to X.Y.Z`

### 4. Create and Push Tag

```bash
git tag v2026.3.0
git push origin main
git push origin v2026.3.0
```

Or combined:

```bash
git push && git push --tags
```

### 5. Monitor CI Build

The `v*` tag push triggers the GitHub Actions release workflow:

1. Go to: `https://github.com/poodle64/thoth/actions`
2. Watch the "Release" workflow run
3. Build takes ~10-15 minutes (Rust compilation, Metal shaders, dylib downloads)

### 6. Review Draft Release

Once CI completes:

1. Go to: `https://github.com/poodle64/thoth/releases`
2. Find the draft release for your version
3. Verify artefacts are present:
   - `Thoth_2026.3.0_aarch64.dmg` - macOS installer
   - `Thoth_2026.3.0_aarch64.app.tar.gz` - Update artefact
   - `Thoth_2026.3.0_aarch64.app.tar.gz.sig` - Signature
   - `latest.json` - Update manifest
4. Download and test the `.dmg` installer locally
5. Edit release notes if needed

### 7. Publish Release

When ready to ship:

1. Click "Edit" on the draft release
2. Review the release notes one final time
3. Uncheck "Set as a pre-release" if checked
4. Click "Publish release"

The release is now live and the update manifest (`latest.json`) becomes available for auto-updates.

## Post-Release

### Update Announcement

Consider announcing the release:

- GitHub Discussions
- Project README
- Social media / community channels

### Monitor Auto-Updates

Users with auto-update enabled will receive the update notification on next app launch. Monitor for issues:

- Check GitHub Issues for update-related problems
- Verify update downloads and installs correctly
- Confirm relaunch works as expected

## Rollback

If critical issues are discovered after release:

### Option 1: Hotfix Release

1. Fix the issue on `main`
2. Bump to next patch version (e.g., `2026.3.0` → `2026.3.1`)
3. Follow normal release process
4. Publish immediately

### Option 2: Delete Release (Not Recommended)

⚠️ **Warning:** Deleting a published release breaks auto-updates for users who already upgraded.

Only delete if:

- No users have upgraded yet (very recent release)
- The issue is critical and prevents app functionality

To delete:

1. Go to release page
2. Click "Delete release"
3. Delete the associated tag: `git push --delete origin vX.Y.Z`

## Troubleshooting

### Build Fails

**Symptoms:** CI workflow fails, no draft release created

**Common causes:**

- Missing `TAURI_SIGNING_PRIVATE_KEY` secret
- Rust compilation error (check `cargo check`)
- Frontend build error (check `pnpm build`)
- Missing sherpa-onnx dylibs (should auto-download via `download-binaries` feature)

**Resolution:**

1. Check workflow logs: Actions → Failed workflow → View logs
2. Fix the issue locally and test
3. Push fix to `main`
4. Delete failed tag: `git push --delete origin vX.Y.Z`
5. Recreate tag and push

### Version Mismatch

**Symptoms:** Release created but version is wrong

**Cause:** Version not updated in all three files

**Resolution:**

1. Delete the tag: `git push --delete origin vX.Y.Z`
2. Fix version in all files
3. Amend commit: `git commit --amend --no-edit`
4. Recreate tag: `git tag -f vX.Y.Z`
5. Force push: `git push -f && git push -f --tags`

### Update Manifest Missing

**Symptoms:** `latest.json` not in release artefacts

**Cause:** `bundle.createUpdaterArtifacts` not set to `true` in `tauri.conf.json`

**Resolution:**

1. Verify `tauri.conf.json` has `"createUpdaterArtifacts": true`
2. Delete failed release and tag
3. Recreate release

## Reference

- GitHub Actions workflow: `.github/workflows/release.yml`
- Deployment docs: `docs/DEPLOYMENT.md`
- Version bump script: `scripts/bump-version.sh`
- Tauri updater plugin: https://v2.tauri.app/plugin/updater/
