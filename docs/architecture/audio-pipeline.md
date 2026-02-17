# Audio Recording and Transcription Pipeline

## Overview

Thoth captures audio from any input device using cpal (cross-platform audio library), converts it to 16kHz mono PCM, and processes it through Whisper transcription with optional AI enhancement.

## Pipeline Stages

```
Microphone → cpal Capture → Ring Buffer → WAV File → Whisper → Text → [AI Enhancement] → Paste
```

### Pipeline State Machine

The pipeline transitions through these states:

| State        | Description                                         |
| ------------ | --------------------------------------------------- |
| Idle         | Ready for recording                                 |
| Recording    | Capturing audio from input device                   |
| Transcribing | Processing WAV file through Whisper                 |
| Filtering    | Applying dictionary replacements and output filters |
| Enhancing    | AI enhancement via Ollama (optional)                |
| Outputting   | Copying to clipboard and/or pasting at cursor       |
| Completed    | Pipeline finished successfully                      |
| Failed       | Pipeline encountered an error                       |

## Audio Capture

### cpal Cross-Platform Audio

`AudioRecorder` uses cpal for device-independent audio capture:

- Enumerates and selects input devices via `cpal::default_host()`
- Supports both default device and user-configured device selection
- Does NOT modify system default audio device
- Real-time callback writes samples to lock-free ring buffer

### Lock-Free Ring Buffer

The `AudioRingBuffer` provides real-time safe audio transfer:

- Pre-allocated 65536 samples (~4 seconds at 16kHz)
- Single-producer single-consumer (SPSC) design
- Atomic indices for lock-free operation
- No allocations in the audio callback path

### Format Conversion (Real-time)

| Step              | Operation                        |
| ----------------- | -------------------------------- |
| Channel mixing    | Multi-channel → Mono (averaging) |
| Sample rate       | Device rate → 16kHz (decimation) |
| Format conversion | Float32 → Int16 PCM              |

Output: 16kHz mono Int16 PCM written to WAV file via hound.

### Writer Thread

A dedicated writer thread:

1. Reads samples from the ring buffer (non-blocking)
2. Downsamples and converts to 16kHz mono i16
3. Writes to WAV file using the hound crate
4. Drains remaining samples when recording stops

## Voice Activity Detection (VAD)

### webrtc-vad Integration

`VoiceActivityDetector` wraps the webrtc-vad library for speech boundary detection:

- Operates at 16kHz sample rate
- Processes audio in 10ms, 20ms, or 30ms frames (default: 30ms = 480 samples)
- Supports four aggressiveness modes

### Aggressiveness Modes

| Mode           | Description                                  |
| -------------- | -------------------------------------------- |
| Quality        | Least aggressive; best for clean audio       |
| LowBitrate     | Low bitrate optimised                        |
| Aggressive     | Default; good for moderate noise             |
| VeryAggressive | Most aggressive; best for noisy environments |

### VAD Configuration

```rust
VadConfig {
    aggressiveness: VadAggressiveness::Aggressive,
    frame_duration: VadFrameDuration::Ms30,
    speech_start_frames: 3,      // 90ms to confirm speech start
    speech_end_frames: 15,       // 450ms to confirm speech end
    pre_speech_padding_ms: 300,  // Capture audio before detected speech
    post_speech_padding_ms: 300, // Capture audio after detected speech
    auto_stop_silence_ms: Some(2000), // Auto-stop after 2s silence
}
```

### VAD State Machine

| State           | Description                             |
| --------------- | --------------------------------------- |
| Silence         | No speech detected                      |
| PossibleSpeech  | Accumulating consecutive speech frames  |
| Speaking        | Speech confirmed and ongoing            |
| PossibleSilence | Accumulating consecutive silence frames |

### VAD Events

| Event             | When Emitted                           |
| ----------------- | -------------------------------------- |
| SpeechStart       | Speech confirmed after threshold met   |
| SpeechEnd         | Silence confirmed after speech ended   |
| AutoStopTriggered | Extended silence after speech detected |

### VadRecorder

`VadRecorder` wraps `AudioRecorder` to add real-time VAD:

- Creates a parallel audio stream for VAD processing
- Processes audio through VAD in a dedicated thread
- Sends events via crossbeam channel to frontend
- Supports auto-stop when silence duration exceeds threshold

## Audio Metering

### Real-time Level Visualisation

`AudioMeter` provides audio levels for UI feedback:

```rust
AudioLevel {
    rms: f32,   // Root mean square, 0.0-1.0
    peak: f32,  // Peak with decay, 0.0-1.0
    db: f32,    // Decibels, typically -60 to 0
}
```

### Peak Decay

- Default decay rate: 0.95 (~300ms peak hold at 30Hz updates)
- Peak tracks maximum sample amplitude
- Decay provides visual "hold" effect for transient peaks

## Transcription

Whisper transcription via whisper.cpp:

- Reads 16kHz mono WAV file
- Processes through configured Whisper model
- Returns transcribed text

## Post-Processing

### Filtering

| Filter                    | Operation                            |
| ------------------------- | ------------------------------------ |
| TranscriptionOutputFilter | Remove tags, brackets, filler words  |
| WordReplacementService    | Apply user-defined word replacements |

### AI Enhancement (Optional)

Enhancement via Ollama (local LLM):

- Grammar and punctuation correction
- Context-aware improvements
- Configurable model and prompt template

## Output

Text insertion at cursor:

1. Copy transcription to clipboard
2. Simulate Cmd+V paste (or character-by-character typing)
3. Restore original clipboard content

## Audio Format Requirements

| Stage          | Sample Rate | Channels | Format    |
| -------------- | ----------- | -------- | --------- |
| Device Input   | Variable    | Variable | Float32   |
| Ring Buffer    | Variable    | Variable | Float32   |
| WAV File       | 16,000 Hz   | Mono     | Int16 PCM |
| Whisper Input  | 16,000 Hz   | Mono     | Int16 PCM |
| VAD Processing | 16,000 Hz   | Mono     | Int16/F32 |

## File Locations

| Type             | Path                   |
| ---------------- | ---------------------- |
| Audio Recordings | `~/.thoth/Recordings/` |
| Temporary VAD    | System temp directory  |
| SQLite Database  | `~/.thoth/`            |

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Audio Capture Layer                              │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌─────────────────┐    ┌──────────────────────────┐ │
│  │   cpal      │───▶│  Audio Callback │───▶│  AudioRingBuffer (SPSC)  │ │
│  │   Stream    │    │  (lock-free)    │    │  65536 samples           │ │
│  └─────────────┘    └─────────────────┘    └────────────┬─────────────┘ │
│                                                         │               │
│  ┌──────────────────────────────────────────────────────┼───────────────┤
│  │                      Writer Thread                   │               │
│  │  ┌─────────────┐    ┌────────────────┐    ┌─────────▼─────────────┐ │
│  │  │ Downsample  │◀───│ Read Buffer    │◀───│ Ring Buffer Consumer  │ │
│  │  │ to 16kHz    │    │ (non-blocking) │    │                       │ │
│  │  └──────┬──────┘    └────────────────┘    └───────────────────────┘ │
│  │         │                                                            │
│  │  ┌──────▼──────┐                                                     │
│  │  │ WAV Writer  │───▶ ~/.thoth/Recordings/thoth_recording_*.wav       │
│  │  │ (hound)     │                                                     │
│  │  └─────────────┘                                                     │
│  └──────────────────────────────────────────────────────────────────────┤
│                                                                          │
├─────────────────────────────────────────────────────────────────────────┤
│                        VAD Processing Layer                              │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌────────────────┐    ┌─────────────────────────┐   │
│  │ VAD Stream  │───▶│ VAD Ring Buffer│───▶│ VoiceActivityDetector   │   │
│  │ (parallel)  │    │                │    │ (webrtc-vad)            │   │
│  └─────────────┘    └────────────────┘    └───────────┬─────────────┘   │
│                                                       │                  │
│                                            ┌──────────▼──────────┐       │
│                                            │ VAD Events Channel  │       │
│                                            │ (crossbeam)         │       │
│                                            └──────────┬──────────┘       │
│                                                       │                  │
│                                            ┌──────────▼──────────┐       │
│                                            │ Frontend (Tauri)    │       │
│                                            └─────────────────────┘       │
├─────────────────────────────────────────────────────────────────────────┤
│                        Metering Layer                                    │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌────────────────┐    ┌─────────────────────────┐   │
│  │ AudioMeter  │───▶│ RMS/Peak Calc  │───▶│ AudioLevel Event        │   │
│  │             │    │                │    │ (Tauri emit)            │   │
│  └─────────────┘    └────────────────┘    └─────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```
