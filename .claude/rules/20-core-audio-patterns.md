---
paths: '**/CoreAudioRecorder.swift,**/Recorder.swift,**/AudioDeviceManager.swift,**/*Audio*.swift'
---

# Thoth Core Audio Patterns

Audio recording patterns using Core Audio AUHAL.

## Core Principle

- Must use Core Audio AUHAL for recording - NOT AVFoundation
- Must NOT change system default audio device
- AUHAL provides low-latency, device-specific access without system-wide effects

## Audio Format Requirements

- Must output **16kHz mono PCM Int16** for whisper.cpp compatibility
- Must handle format conversion from device native format (e.g., 48kHz stereo Float32)
- Pipeline: Device Format → Channel Mixing → Sample Rate Conversion → Int16 PCM

## Thread Safety

- Must use `NSLock` for thread-safe metering access (not actors)
- Audio callbacks run on real-time threads - must be lock-protected
- Must provide thread-safe `averagePower` and `peakPower` properties (audio metres)

## Memory Management

- Must pre-allocate all buffers BEFORE starting audio unit
- Must NEVER allocate memory in real-time audio callbacks
- Must deallocate buffers in `stopRecording()` after stopping
- Must call `stopRecording()` in `deinit` to ensure cleanup

## Device Management

- Must validate device availability before recording
- Must check `kAudioDevicePropertyDeviceIsAlive` for device validity
- Must support mid-recording device switching without file interruption
- Must handle device ID of 0 as invalid

## Resource Cleanup

`stopRecording()` must clean up in order:

1. Stop and dispose AudioUnit
2. Close audio file (ExtAudioFileDispose)
3. Free conversion buffer
4. Free render buffer
5. Reset state flags
6. Reset metres (thread-safe)

## Error Handling

- Must define `CoreAudioRecorderError` enum with `LocalizedError` conformance
- Must include OSStatus in error cases for debugging
- Must provide `recoverySuggestion` for user-actionable errors

## Logging

- Must use logger with category `"CoreAudioRecorder"`
- Must log device details on recording start for debugging
- Must log start/stop events with descriptive emoji markers

## Key Directives

- **Use Core Audio AUHAL** - never AVFoundation for recording
- **Output 16kHz mono PCM Int16** for transcription compatibility
- **Pre-allocate all buffers** before starting audio unit
- **Use `NSLock`** for thread-safe metre access
- **Validate device availability** before recording
- **Clean up all resources** in `stopRecording()` and `deinit`
- **Never allocate memory** in real-time audio callbacks

## See Also

- [docs/architecture/audio-pipeline.md](../../docs/architecture/audio-pipeline.md) - Complete pipeline documentation
