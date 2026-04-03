# Screenshot Thoth Windows

Capture screenshots of running Thoth windows for documentation.

**Arguments**: $ARGUMENTS

## Prerequisites

- Thoth must be running (via `pnpm tauri dev` or `/Applications/Thoth.app`)
- Navigate to the desired tab/view before running this command

## Steps

### 1. Create output directory

```bash
mkdir -p docs/branding
```

### 2. Discover Thoth windows

Run the Swift helper to find all Thoth windows with their IDs and names:

```bash
swift scripts/thoth-windows.swift
```

This returns JSON with each window's `id`, `name`, `width`, `height`, and `onScreen` status.

### 3. Parse arguments and determine what to capture

The `$ARGUMENTS` value controls what gets captured:

| Argument      | Behaviour                                  |
| ------------- | ------------------------------------------ |
| _(empty)_     | Capture all named windows (main + history) |
| `main`        | Capture only the main Thoth window         |
| `history`     | Capture only the History window            |
| `all`         | Capture all windows including unnamed ones |
| A window name | Capture the window matching that name      |

### 4. Capture each target window

For each window to capture, run:

```bash
screencapture -l <windowID> -o -x docs/branding/<filename>.png
```

**Filename convention**: derive from the window name:

- `"Thoth"` → `main.png`
- `"Thoth - History"` → `history.png`
- Unnamed windows → `window-<id>.png`
- If `$ARGUMENTS` contains a custom filename (e.g., `main overview-tab`), use the second word as the filename stem instead

The `-o` flag excludes window shadows. The `-x` flag suppresses the shutter sound.

### 5. Verify captures

Read each captured PNG to verify it captured correctly. Show the image to confirm the content is what was expected.

### 6. Report

List all captured screenshots with their file paths and dimensions:

```
docs/branding/main.png (900x800)
docs/branding/history.png (800x600)
```

## Notes

- `screencapture -l` works even on off-screen or hidden windows
- Window IDs change each time the app restarts; always re-discover them
- The Swift helper at `scripts/thoth-windows.swift` filters out tiny windows (menu bar icon, overlays) by default
- For Retina displays, captured PNGs will be 2x the logical dimensions
- To capture a specific tab, navigate to it in the app first, then run this command
