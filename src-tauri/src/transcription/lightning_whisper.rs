//! Lightning Whisper MLX backend
//!
//! Uses the lightning-whisper-mlx Python package for fast Whisper transcription
//! on Apple Silicon via MLX. Runs as a subprocess, not a native Rust library.

use anyhow::{anyhow, Result};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Transcription service using Lightning Whisper MLX (Python subprocess)
pub struct LightningWhisperTranscriptionService {
    model: String,
    quant: Option<String>,
}

impl LightningWhisperTranscriptionService {
    /// Create a new Lightning Whisper MLX transcription service
    pub fn new(model: &str, quant: Option<&str>) -> Self {
        Self {
            model: model.to_string(),
            quant: quant.map(|q| q.to_string()),
        }
    }

    /// Check if lightning-whisper-mlx is importable via Python
    pub fn is_available() -> bool {
        Command::new("python3")
            .args(["-c", "import lightning_whisper_mlx"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Transcribe audio from a WAV file using Lightning Whisper MLX
    pub fn transcribe(&self, audio_path: &Path) -> Result<String> {
        let quant_repr = match &self.quant {
            Some(q) => format!("\"{}\"", q),
            None => "None".to_string(),
        };

        let script = format!(
            r#"from lightning_whisper_mlx import TranscriptionModel; m = TranscriptionModel(model="{model}", batch_size=12, quant={quant}); import sys; result = m.transcribe(sys.argv[1]); print(result["text"])"#,
            model = self.model,
            quant = quant_repr,
        );

        let audio_path_str = audio_path
            .to_str()
            .ok_or_else(|| anyhow!("Invalid audio path"))?;

        tracing::info!(
            "Lightning Whisper MLX: transcribing {} with model={}, quant={:?}",
            audio_path_str,
            self.model,
            self.quant
        );

        let mut child = Command::new("python3")
            .args(["-c", &script, audio_path_str])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("Failed to start Lightning Whisper MLX: {}", e))?;

        let timeout = Duration::from_secs(30);
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        return Err(anyhow!("Lightning Whisper MLX timed out after 30 seconds"));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(anyhow!("Error waiting for Lightning Whisper MLX: {}", e)),
            }
        }

        let output = child
            .wait_with_output()
            .map_err(|e| anyhow!("Failed to read Lightning Whisper MLX output: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "Lightning Whisper MLX failed (exit {}): {}",
                output.status,
                stderr.trim()
            ));
        }

        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        tracing::info!("Lightning Whisper MLX transcription complete: {} chars", text.len());
        Ok(text)
    }
}

/// Tauri command: Check if Lightning Whisper MLX is available
#[tauri::command]
pub fn is_lightning_whisper_available() -> bool {
    LightningWhisperTranscriptionService::is_available()
}

/// Tauri command: Install lightning-whisper-mlx via pip in a terminal window.
///
/// Opens a new Terminal.app window (macOS) so the user can see install progress.
/// Returns immediately; the install runs in the background terminal.
#[tauri::command]
pub fn install_lightning_whisper_mlx() -> Result<(), String> {
    let script = "pip install lightning-whisper-mlx; echo ''; echo '✅ Done. You can close this window.'; read -p 'Press Enter to close...'";

    #[cfg(target_os = "macos")]
    {
        // Use osascript to open Terminal with the install command
        let osa = format!(
            r#"tell application "Terminal"
    do script "{}"
    activate
end tell"#,
            script.replace('"', "\\\"")
        );
        std::process::Command::new("osascript")
            .args(["-e", &osa])
            .spawn()
            .map_err(|e| format!("Failed to open Terminal: {}", e))?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Linux fallback: try x-terminal-emulator or xterm
        let tried = std::process::Command::new("x-terminal-emulator")
            .args(["-e", &format!("bash -c '{script}'")])
            .spawn();
        if tried.is_err() {
            std::process::Command::new("xterm")
                .args(["-e", &format!("bash -c '{script}'")])
                .spawn()
                .map_err(|e| format!("Failed to open terminal: {}", e))?;
        }
    }

    tracing::info!("Lightning Whisper MLX install triggered in external terminal");
    Ok(())
}
