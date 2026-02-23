# Recording Indicator Audio Equalizer - RESOLVED ✅

## Issue Summary

The native indicator was running but the waveform wasn't displaying.

## Root Causes

### Issue 1: State Management & Audio Metering

1. **Audio metering never started**: The old WebView flow waited for `indicator_window_ready()` signal before starting metering. With native rendering, there's no window to signal readiness, so metering was never started.

2. **State stuck in Idle**: The indicator was initialised in Idle state and never transitioned to Recording, so the waveform rendering code path was never executed.

3. **State overwritten by audio updates**: The `update_audio()` method was hardcoded to set state to Recording on every audio level update, preventing the Processing state from ever showing.

### Issue 2: Native Indicator Never Initialised (Critical)

The most critical bug: `show_indicator_instant()` (called by shortcuts/tray) only used the WebView-based indicator, never trying the native renderer. The native indicator logic existed in `show_recording_indicator()`, but that function was never called!

Flow was:

1. User presses shortcut → `show_indicator_instant()` (WebView only)
2. Recording starts → tries to set native state → "Native indicator not initialised" error
3. Audio metering sends updates → "Native indicator not initialised" error
4. User sees WebView indicator (the "dotted lines") with no waveform

### Issue 3: Thread Safety Violation (Crash)

After fixing Issue 2, the app crashed with "Rust cannot catch foreign exceptions". The keyboard service calls `show_indicator_instant()` from a background polling thread, which was trying to create NSWindow directly.

**macOS NSWindow MUST be created on the main thread.** Violating this causes an uncatchable Objective-C exception that crashes the app.

## Solution

**Commit 7ed5d0b - State Management:**

- Start audio metering immediately when recording begins (don't wait for signal)
- Set native indicator state to Recording when recording starts
- Set state to Processing when transcription begins
- Remove hardcoded state override in `update_audio()` - it should only update audio levels, not manage state
- Reset state to Idle when hiding indicator

**Commit f00b79b - Initialisation (Critical):**

- Added native indicator initialisation to `show_indicator_instant()`
- Made `calculate_indicator_position()` generic to work with both code paths
- Native indicator now properly initialises, shows, and receives audio updates

**Commit 0c4531d - Thread Safety (Crash Fix):**

- Dispatch NSWindow creation to main thread via `tauri::async_runtime::spawn()`
- Wait briefly (100ms timeout) for initialisation to complete
- Fall back to WebView if initialisation fails or times out
- Same pattern as audio level updates - all NSWindow operations must go through main thread

## Files Changed

**State Management (7ed5d0b):**

- [src-tauri/src/pipeline.rs](src-tauri/src/pipeline.rs#L231-L248) - Start metering immediately, set Recording state
- [src-tauri/src/pipeline.rs](src-tauri/src/pipeline.rs#L424-L434) - Set Processing state during transcription
- [src-tauri/src/recording_indicator/native.rs](src-tauri/src/recording_indicator/native.rs#L579-L585) - Remove state override in update_audio()
- [src-tauri/src/recording_indicator/native.rs](src-tauri/src/recording_indicator/native.rs#L574-L577) - Reset to Idle on hide
- [src-tauri/src/audio/preview.rs](src-tauri/src/audio/preview.rs#L179-L181) - Fixed unused import warnings

**Initialisation (f00b79b):**

- [src-tauri/src/recording_indicator.rs](src-tauri/src/recording_indicator.rs#L635-L705) - Added native support to show_indicator_instant()
- [src-tauri/src/recording_indicator.rs](src-tauri/src/recording_indicator.rs#L461) - Made calculate_indicator_position() generic

**Thread Safety (0c4531d):**

- [src-tauri/src/recording_indicator.rs](src-tauri/src/recording_indicator.rs#L554-L615) - Dispatch native init to main thread with timeout

## Result

✅ Native indicator now initialises correctly via `show_indicator_instant()`
✅ NSWindow created safely on main thread (no more crashes)
✅ Waveform displays correctly in both pill and dot styles
✅ Transitions smoothly: Recording → Processing → Idle
✅ Audio levels update in real-time
✅ No more "stuck in weird state" when toggling quickly
✅ No more "Native indicator not initialised" errors
✅ No more "Rust cannot catch foreign exceptions" crashes

## Testing

Build and run:

```bash
pnpm tauri dev
```

Test the waveform:

1. Start recording with configured shortcut
2. Speak into microphone - waveform should pulse with audio levels
3. Stop recording - indicator should show processing animation
4. When complete, indicator should disappear

Expected behaviour:

- Pill style: Animated waveform bars on the right side
- Dot style: Pulsing glow around the icon

Check logs for successful initialisation:

```bash
tail -f ~/.thoth/logs/thoth-debug.log | grep -i "native indicator"
```

Should see:

- "Native indicator shown at (x, y)"
- No "not initialised" errors
- No crashes
