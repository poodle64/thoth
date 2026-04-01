# Deploy Thoth

Build, install, and launch Thoth locally with clean macOS permissions.

## Steps

1. **Quit running instance** - Gracefully quit Thoth if it's running:

   ```bash
   osascript -e 'tell application "Thoth" to quit' 2>/dev/null; sleep 1
   ```

2. **Build production binary** - Run the Tauri build:

   ```bash
   pnpm tauri build
   ```

   This produces `src-tauri/target/release/bundle/macos/Thoth.app`

3. **Install to /Applications** - Sync the built bundle into place (rsync --delete replaces all files cleanly, unlike cp -R which merges into existing .app bundles):

   ```bash
   rsync -a --delete src-tauri/target/release/bundle/macos/Thoth.app/ /Applications/Thoth.app/
   ```

4. **Clear quarantine** - Remove macOS quarantine extended attributes:

   ```bash
   xattr -cr /Applications/Thoth.app
   ```

5. **Reset TCC permissions** - Clear stale Microphone, Accessibility, and Input Monitoring grants so macOS re-prompts cleanly:

   ```bash
   tccutil reset Microphone com.poodle64.thoth
   tccutil reset Accessibility com.poodle64.thoth
   tccutil reset ListenEvent com.poodle64.thoth
   ```

6. **Launch** - Open the freshly installed app:

   ```bash
   open /Applications/Thoth.app
   ```

7. **Report** - Confirm the app launched and remind user to grant Microphone and Accessibility permissions when prompted.

## Notes

- Bundle identifier: `com.poodle64.thoth`
- macOS caches TCC permissions per code signature; rebuilding changes the signature, so stale grants cause silent failures
- The quarantine attribute (`com.apple.quarantine`) triggers Gatekeeper warnings on unsigned builds
- If the build fails, check `pnpm install` was run and Rust toolchain is current
