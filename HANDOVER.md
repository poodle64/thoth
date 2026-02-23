# Recording Indicator Audio Equalizer - IN PROGRESS

## Current Status

The backend is working correctly, but the frontend isn't displaying the audio visualizer.

## What's Working ✅

1. **`indicator_window_ready()` handshake** - Frontend calls command when recording starts (via `pipeline-progress` listener)
2. **Backend metering** - Audio metering starts successfully
3. **Event emission** - Backend successfully emits `recording-audio-level` events to indicator window

Evidence from logs:

```
DEBUG thoth_lib::pipeline: Indicator window signaled ready
INFO thoth_lib::pipeline: Indicator ready and recording in progress - starting metering
INFO thoth_lib::audio::preview: Recording metering started
DEBUG thoth_lib::audio::preview: Successfully emitted first event to indicator window
```

## What's Broken ❌

The frontend event listener for `recording-audio-level` isn't updating the UI. Events are reaching the window but the visualizer doesn't animate.

## Investigation

**Backend (Confirmed Working):**

- [src-tauri/src/pipeline.rs:324-342](src-tauri/src/pipeline.rs#L324-L342) - `indicator_window_ready()` command
- [src-tauri/src/audio/preview.rs:174-330](src-tauri/src/audio/preview.rs#L174-L330) - Metering emits events

**Frontend (Suspected Issue):**

- [src/routes/(indicator)/recording-indicator/+page.svelte:109-122](<src/routes/(indicator)/recording-indicator/+page.svelte#L109-L122>) - Event listener setup
- [src/routes/(indicator)/recording-indicator/+page.svelte:76-91](<src/routes/(indicator)/recording-indicator/+page.svelte#L76-L98>) - Calls `indicator_window_ready()` on recording state

## Possible Causes

1. **Event listener not registering** - Despite `listen()` being called, callback may not be attached
2. **Cross-window event delivery issue** - Tauri may have issues delivering events to specific windows
3. **Frontend state not updating** - Callback runs but Svelte $state reactivity isn't triggering
4. **Build cache issue** - Frontend changes not being picked up by dev server

## Next Steps

1. **Verify frontend rebuild** - Restart `pnpm tauri dev` to ensure latest code is running
2. **Check browser console** - Open DevTools on indicator window to see if listener errors exist
3. **Test global event** - Change backend to emit globally instead of to specific window
4. **Add debugging** - More logging in frontend callback to confirm it's executing

## Test Instructions

1. Kill any running `pnpm tauri dev`
2. `pnpm run build` (rebuild frontend)
3. `pnpm tauri dev` (start fresh)
4. Open DevTools on main window (Cmd+Option+I)
5. Start recording
6. Check console for `[Indicator]` messages
7. Check backend logs: `tail -f ~/.thoth/logs/thoth-debug.log`

## Key Files

- [src/routes/(indicator)/recording-indicator/+page.svelte](<src/routes/(indicator)/recording-indicator/+page.svelte>) - Frontend
- [src-tauri/src/audio/preview.rs](src-tauri/src/audio/preview.rs) - Backend metering
- [src-tauri/src/pipeline.rs](src-tauri/src/pipeline.rs) - Handshake command
