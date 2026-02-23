# Recording Indicator Audio Equalizer - RESOLVED ✅

## Issue Summary

The native indicator was running but the waveform wasn't displaying.

## Root Causes

1. **Audio metering never started**: The old WebView flow waited for `indicator_window_ready()` signal before starting metering. With native rendering, there's no window to signal readiness, so metering was never started.

2. **State stuck in Idle**: The indicator was initialised in Idle state and never transitioned to Recording, so the waveform rendering code path was never executed.

3. **State overwritten by audio updates**: The `update_audio()` method was hardcoded to set state to Recording on every audio level update, preventing the Processing state from ever showing.

## Solution

**Commit: 7ed5d0b**

- Start audio metering immediately when recording begins (don't wait for signal)
- Set native indicator state to Recording when recording starts
- Set state to Processing when transcription begins
- Remove hardcoded state override in `update_audio()` - it should only update audio levels, not manage state
- Reset state to Idle when hiding indicator

## Files Changed

- [src-tauri/src/pipeline.rs](src-tauri/src/pipeline.rs#L231-L248) - Start metering immediately, set Recording state
- [src-tauri/src/pipeline.rs](src-tauri/src/pipeline.rs#L424-L434) - Set Processing state during transcription
- [src-tauri/src/recording_indicator/native.rs](src-tauri/src/recording_indicator/native.rs#L579-L585) - Remove state override in update_audio()
- [src-tauri/src/recording_indicator/native.rs](src-tauri/src/recording_indicator/native.rs#L574-L577) - Reset to Idle on hide
- [src-tauri/src/audio/preview.rs](src-tauri/src/audio/preview.rs#L179-L181) - Fixed unused import warnings

## Result

✅ Waveform now displays correctly in both pill and dot styles
✅ Transitions smoothly: Recording → Processing → Idle
✅ Audio levels update in real-time
✅ No more "stuck in weird state" when toggling quickly

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
