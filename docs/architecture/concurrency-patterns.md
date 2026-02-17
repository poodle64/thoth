# Concurrency Patterns

Detailed async/await, thread safety, and synchronisation patterns for Thoth's Rust backend.

## Overview

Thoth uses a hybrid concurrency model:

- **Tokio async runtime** for I/O-bound operations (network, file I/O, timers)
- **Dedicated threads** for real-time audio capture (avoiding async overhead)
- **parking_lot** for efficient synchronisation (Mutex, RwLock)
- **Atomic types** for lock-free state flags
- **OnceLock** for lazy static initialisation
- **Tauri events** for frontend communication

## State Machine Pattern

### Pipeline State Enum

Use an explicit state enum for the transcription pipeline lifecycle:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineState {
    Idle,
    Recording,
    Transcribing,
    Filtering,
    Enhancing,
    Outputting,
    Completed,
    Failed,
}
```

### State Transitions

```
idle → recording → transcribing → [filtering] → [enhancing] → outputting → completed
  ↑                                                                            ↓
  └───────────────────────────── (error or cancel) ───────────────────────────┘
```

### Atomic State Flags

Use `AtomicBool` for simple boolean state that needs lock-free access:

```rust
use std::sync::atomic::{AtomicBool, Ordering};

/// Track if pipeline is currently running
static PIPELINE_RUNNING: AtomicBool = AtomicBool::new(false);

/// Start recording with atomic state check
#[tauri::command]
pub fn pipeline_start_recording(app: AppHandle) -> Result<String, String> {
    // Atomically swap to true, returning previous value
    if PIPELINE_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Pipeline is already running".to_string());
    }

    // ... start recording logic ...
    Ok(path)
}

/// Check pipeline state without locking
#[tauri::command]
pub fn is_pipeline_running() -> bool {
    PIPELINE_RUNNING.load(Ordering::SeqCst)
}
```

## Lazy Static Initialisation with OnceLock

### Global Service Pattern

Use `OnceLock` for thread-safe lazy initialisation of global services:

```rust
use parking_lot::Mutex;
use std::sync::OnceLock;

/// Global transcription service instance
static TRANSCRIPTION_SERVICE: OnceLock<Mutex<Option<TranscriptionService>>> = OnceLock::new();

fn get_service() -> &'static Mutex<Option<TranscriptionService>> {
    TRANSCRIPTION_SERVICE.get_or_init(|| Mutex::new(None))
}

/// Initialise the transcription service
#[tauri::command]
pub fn init_whisper_transcription(model_path: String) -> Result<(), String> {
    let service = TranscriptionService::new_whisper(&PathBuf::from(model_path))
        .map_err(|e| e.to_string())?;

    let mut guard = get_service().lock();
    *guard = Some(service);

    tracing::info!("Whisper transcription service initialised");
    Ok(())
}

/// Use the service
#[tauri::command]
pub fn transcribe_file(audio_path: String) -> Result<String, String> {
    let mut guard = get_service().lock();
    let service = guard
        .as_mut()
        .ok_or_else(|| "Transcription service not initialised".to_string())?;

    service
        .transcribe(&PathBuf::from(audio_path))
        .map_err(|e| e.to_string())
}
```

### Manager State Pattern

Combine `OnceLock` with `RwLock` for read-heavy state:

```rust
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Global shortcut manager instance
static MANAGER: OnceLock<RwLock<ShortcutManagerState>> = OnceLock::new();

struct ShortcutManagerState {
    shortcuts: HashMap<String, ShortcutInfo>,
}

fn get_manager() -> &'static RwLock<ShortcutManagerState> {
    MANAGER.get_or_init(|| RwLock::new(ShortcutManagerState::new()))
}

/// Read-heavy operation: list shortcuts
pub fn list_registered() -> Vec<ShortcutInfo> {
    let manager = get_manager().read();  // Multiple readers allowed
    manager.shortcuts.values().cloned().collect()
}

/// Write operation: register shortcut
pub fn register(id: String, info: ShortcutInfo) {
    let mut manager = get_manager().write();  // Exclusive access
    manager.shortcuts.insert(id, info);
}
```

## parking_lot Synchronisation

### Why parking_lot over std::sync

parking_lot provides:

- No poisoning (panics don't leave locks unusable)
- Smaller lock sizes
- Faster lock acquisition
- Fair locking options

### Mutex for Exclusive Access

```rust
use parking_lot::Mutex;

/// Global transcription service with exclusive access
static TRANSCRIPTION_SERVICE: OnceLock<Mutex<Option<TranscriptionService>>> = OnceLock::new();

fn get_service() -> &'static Mutex<Option<TranscriptionService>> {
    TRANSCRIPTION_SERVICE.get_or_init(|| Mutex::new(None))
}

/// Access with automatic unlocking
pub fn transcribe_file(audio_path: String) -> Result<String, String> {
    let mut guard = get_service().lock();  // Blocks until lock acquired
    // guard automatically unlocks when dropped
    // ...
}
```

### RwLock for Read-Heavy Workloads

```rust
use parking_lot::RwLock;

static MANAGER: OnceLock<RwLock<ShortcutManagerState>> = OnceLock::new();

/// Multiple readers can access simultaneously
pub fn list_registered() -> Vec<ShortcutInfo> {
    let manager = get_manager().read();
    manager.shortcuts.values().cloned().collect()
}

/// Writers get exclusive access
pub fn unregister(id: &str) -> Result<(), String> {
    let mut manager = get_manager().write();
    manager.shortcuts.remove(id);
    Ok(())
}
```

## Audio Thread Handling

### Lock-Free Ring Buffer Pattern

Audio callbacks run on real-time threads; avoid locks and allocations:

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct AudioRecorder {
    stream: Option<cpal::Stream>,
    writer_handle: Option<std::thread::JoinHandle<Result<()>>>,
    stop_signal: Arc<AtomicBool>,
    ring_buffer: Arc<AudioRingBuffer>,
}

impl AudioRecorder {
    pub fn start(&mut self, device: &cpal::Device, output_path: &Path) -> Result<()> {
        // Reset stop signal
        self.stop_signal.store(false, Ordering::SeqCst);
        self.ring_buffer = Arc::new(AudioRingBuffer::new());

        // Clone for writer thread
        let ring_buffer = self.ring_buffer.clone();
        let stop_signal = self.stop_signal.clone();

        // Spawn dedicated writer thread
        self.writer_handle = Some(std::thread::spawn(move || {
            write_audio_to_file(ring_buffer, &writer_path, stop_signal)
        }));

        // Clone for audio callback
        let callback_buffer = self.ring_buffer.clone();

        // Build input stream with lock-free callback
        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // LOCK-FREE: Ring buffer write does not allocate
                let written = callback_buffer.write(data);
                if written < data.len() {
                    tracing::warn!("Audio buffer overflow");
                }
            },
            |err| tracing::error!("Audio stream error: {}", err),
            None,
        )?;

        stream.play()?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<PathBuf> {
        // Signal writer to stop
        self.stop_signal.store(true, Ordering::SeqCst);

        // Stop stream first
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }

        // Wait for writer thread
        if let Some(handle) = self.writer_handle.take() {
            handle.join().map_err(|_| anyhow!("Writer thread panicked"))??;
        }

        // ...
    }
}
```

### Writer Thread Pattern

Dedicated thread reads from ring buffer and writes to file:

```rust
fn write_audio_to_file(
    ring_buffer: Arc<AudioRingBuffer>,
    path: &Path,
    stop_signal: Arc<AtomicBool>,
) -> Result<()> {
    let mut writer = hound::WavWriter::create(path, spec)?;
    let mut read_buffer = vec![0.0f32; 4096];

    // Poll until stop signal
    while !stop_signal.load(Ordering::SeqCst) {
        let read = ring_buffer.read(&mut read_buffer);
        if read > 0 {
            let processed = downsample_and_convert(&read_buffer[..read]);
            for sample in &processed {
                writer.write_sample(*sample)?;
            }
        } else {
            // No data; sleep briefly to avoid spinning
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    // Drain remaining samples
    loop {
        let read = ring_buffer.read(&mut read_buffer);
        if read == 0 { break; }
        // ... process remaining samples
    }

    writer.finalize()?;
    Ok(())
}
```

## Tokio Async Runtime

### Async Tauri Commands

Use `async` for I/O-bound Tauri commands:

```rust
/// Stop recording and run the full transcription pipeline
#[tauri::command]
pub async fn pipeline_stop_and_process(
    app: AppHandle,
    config: Option<PipelineConfig>,
) -> Result<PipelineResult, String> {
    let config = config.unwrap_or_default();

    // Stop recording (sync operation)
    let audio_path = crate::audio::stop_recording()?;

    // Run async processing pipeline
    let result = process_audio(&app, &audio_path, &config).await;

    // Mark pipeline as not running
    PIPELINE_RUNNING.store(false, Ordering::SeqCst);

    // Emit completion event
    if let Ok(r) = &result {
        app.emit("pipeline-complete", r).ok();
    }

    result
}
```

### Async Pipeline Processing

```rust
async fn process_audio(
    app: &AppHandle,
    audio_path: &str,
    config: &PipelineConfig,
) -> Result<PipelineResult, String> {
    // 1. Transcribe (sync - CPU-bound)
    emit_progress(app, PipelineState::Transcribing, "Transcribing audio...");
    let raw_text = transcription::transcribe_file(audio_path.to_string())?;

    // 2. Apply filtering (sync - fast)
    let mut text = raw_text.clone();
    if config.apply_filtering {
        text = transcription::filter_transcription(text, None);
    }

    // 3. AI Enhancement (async - network I/O)
    let is_enhanced = if config.enhancement_enabled {
        emit_progress(app, PipelineState::Enhancing, "Enhancing with AI...");
        match enhancement::enhance_text(
            text.clone(),
            config.enhancement_model.clone(),
            config.enhancement_prompt.clone(),
        ).await {
            Ok(enhanced) => {
                text = enhanced;
                true
            }
            Err(e) => {
                tracing::warn!("Enhancement failed: {}", e);
                false
            }
        }
    } else {
        false
    };

    // 4. Output with small delay
    if config.auto_paste {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        crate::text_insert::insert_text_by_paste(text.clone(), None)?;
    }

    Ok(PipelineResult { text, is_enhanced, /* ... */ })
}
```

## Tauri Event System

### Emitting Events to Frontend

Use Tauri's event system for frontend communication:

```rust
use tauri::{AppHandle, Emitter};

/// Progress event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineProgress {
    pub state: PipelineState,
    pub message: String,
}

/// Emit pipeline progress to frontend
fn emit_progress(app: &AppHandle, state: PipelineState, message: &str) {
    let progress = PipelineProgress {
        state,
        message: message.to_string(),
    };
    if let Err(e) = app.emit("pipeline-progress", &progress) {
        tracing::warn!("Failed to emit pipeline progress: {}", e);
    }
}
```

### Shortcut Event Pattern

Emit events from global shortcut handlers:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ShortcutEvent {
    pub id: String,
    pub state: String,  // "pressed" or "released"
}

pub fn register<R: Runtime>(
    app: &AppHandle<R>,
    id: String,
    accelerator: String,
    description: String,
) -> Result<(), String> {
    let shortcut_id = id.clone();
    let app_handle = app.clone();

    global_shortcut
        .on_shortcut(accelerator.as_str(), move |_app, _shortcut, event| {
            let state_str = match event.state {
                ShortcutState::Pressed => "pressed",
                ShortcutState::Released => "released",
            };

            let shortcut_event = ShortcutEvent {
                id: shortcut_id.clone(),
                state: state_str.to_string(),
            };

            match event.state {
                ShortcutState::Pressed => {
                    // Emit legacy event for backwards compatibility
                    app_handle.emit("shortcut-triggered", shortcut_id.clone()).ok();
                    // Emit new event with full state
                    app_handle.emit("shortcut-pressed", &shortcut_event).ok();
                }
                ShortcutState::Released => {
                    app_handle.emit("shortcut-released", &shortcut_event).ok();
                }
            }
        })
        .map_err(|e| format!("Failed to register shortcut: {}", e))?;

    Ok(())
}
```

## Thread Safety Summary

| Context              | Approach                | Example               |
| -------------------- | ----------------------- | --------------------- |
| Global service state | `OnceLock<Mutex<T>>`    | Transcription service |
| Read-heavy state     | `OnceLock<RwLock<T>>`   | Shortcut manager      |
| Boolean flags        | `AtomicBool`            | Pipeline running flag |
| Audio callbacks      | Lock-free ring buffer   | Audio capture         |
| Background I/O       | Dedicated `std::thread` | WAV file writer       |
| Network I/O          | Tokio async             | AI enhancement        |
| Frontend updates     | Tauri events            | Progress updates      |

## Memory Ordering

Use appropriate memory ordering for atomics:

| Ordering            | Use Case                                       |
| ------------------- | ---------------------------------------------- |
| `Ordering::SeqCst`  | Default; provides sequential consistency       |
| `Ordering::Acquire` | Load; synchronises with Release store          |
| `Ordering::Release` | Store; synchronises with Acquire load          |
| `Ordering::Relaxed` | Statistics counters; no synchronisation needed |

For most cases in Thoth, `SeqCst` is used for simplicity and correctness:

```rust
// Stop signal pattern
stop_signal.store(true, Ordering::SeqCst);   // Writer
stop_signal.load(Ordering::SeqCst)            // Reader
```

## See Also

- [audio-pipeline.md](audio-pipeline.md) - Audio recording flow
- [service-patterns.md](service-patterns.md) - Service architecture patterns
