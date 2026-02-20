//! Linux-specific platform functionality
//!
//! Provides Linux implementations for platform-specific features:
//! - Microphone access via PulseAudio/PipeWire
//! - Accessibility (not needed on Linux, kept for API compatibility)
//! - GPU detection for CUDA, HIP/ROCm, and Vulkan backends

use serde::{Deserialize, Serialize};
use std::process::Command;

/// GPU backend type for whisper.cpp
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuBackend {
    /// NVIDIA CUDA
    Cuda,
    /// AMD HIP/ROCm
    Hipblas,
    /// Vulkan (cross-platform)
    Vulkan,
    /// CPU only
    Cpu,
}

impl std::fmt::Display for GpuBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuBackend::Cuda => write!(f, "CUDA"),
            GpuBackend::Hipblas => write!(f, "HIP/ROCm"),
            GpuBackend::Vulkan => write!(f, "Vulkan"),
            GpuBackend::Cpu => write!(f, "CPU"),
        }
    }
}

/// Information about a detected GPU
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    /// GPU backend type
    pub backend: GpuBackend,
    /// GPU name/model
    pub name: String,
    /// VRAM in MB (if available)
    pub vram_mb: Option<u64>,
    /// Whether this GPU is available and functional
    pub available: bool,
}

/// Result of GPU detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuDetectionResult {
    /// List of detected GPUs
    pub gpus: Vec<GpuInfo>,
    /// Which GPU backend was compiled into this build
    pub compiled_backend: GpuBackend,
    /// Recommended backend based on available hardware
    pub recommended_backend: GpuBackend,
}

/// Detect available GPUs on the system
pub fn detect_gpus() -> GpuDetectionResult {
    let mut gpus = Vec::new();

    // Detect NVIDIA GPUs
    if let Some(nvidia) = detect_nvidia_gpu() {
        gpus.push(nvidia);
    }

    // Detect AMD GPUs
    if let Some(amd) = detect_amd_gpu() {
        gpus.push(amd);
    }

    // Detect Vulkan support
    if let Some(vulkan) = detect_vulkan_support() {
        gpus.push(vulkan);
    }

    // Determine compiled backend
    let compiled_backend = get_compiled_backend();

    // Determine recommended backend
    let recommended_backend = determine_recommended_backend(&gpus, compiled_backend);

    GpuDetectionResult {
        gpus,
        compiled_backend,
        recommended_backend,
    }
}

/// Get the GPU backend that was compiled into this build
pub fn get_compiled_backend() -> GpuBackend {
    #[cfg(feature = "cuda")]
    {
        GpuBackend::Cuda
    }
    #[cfg(all(not(feature = "cuda"), feature = "hipblas"))]
    {
        GpuBackend::Hipblas
    }
    #[cfg(all(not(any(feature = "cuda", feature = "hipblas")), feature = "vulkan"))]
    {
        GpuBackend::Vulkan
    }
    #[cfg(not(any(feature = "cuda", feature = "hipblas", feature = "vulkan")))]
    {
        GpuBackend::Cpu
    }
}

/// Detect an NVIDIA GPU by querying `nvidia-smi` for name and VRAM.
fn detect_nvidia_gpu() -> Option<GpuInfo> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let line = stdout.lines().next()?;

            // Parse "GPU Name, 8192" format
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 2 {
                let name = parts[0].trim().to_string();
                let vram_mb = parts[1].trim().parse::<u64>().ok();

                tracing::info!(
                    "Detected NVIDIA GPU: {} ({} MB)",
                    name,
                    vram_mb.unwrap_or(0)
                );

                return Some(GpuInfo {
                    backend: GpuBackend::Cuda,
                    name,
                    vram_mb,
                    available: true,
                });
            }
            None
        }
        Ok(output) => {
            // nvidia-smi exists but returned error - GPU might be in bad state
            tracing::debug!("nvidia-smi returned error: {:?}", output.stderr);
            None
        }
        Err(e) => {
            tracing::debug!("nvidia-smi not available: {}", e);
            None
        }
    }
}

/// Detect an AMD GPU via `rocm-smi` or `hipconfig`.
fn detect_amd_gpu() -> Option<GpuInfo> {
    // Try rocm-smi first (more detailed)
    if let Some(gpu) = detect_amd_via_rocm_smi() {
        return Some(gpu);
    }

    // Fallback to hipconfig
    detect_amd_via_hipconfig()
}

/// Detect an AMD GPU by querying `rocm-smi` for product name and VRAM.
fn detect_amd_via_rocm_smi() -> Option<GpuInfo> {
    let output = Command::new("rocm-smi")
        .args(["--showproductname", "--showmeminfo", "vram"])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Parse output to find GPU name and VRAM
            let mut name = String::new();
            let mut vram_mb: Option<u64> = None;

            for line in stdout.lines() {
                if line.contains("Card") && line.contains("series") {
                    name = line.trim().to_string();
                }
                // VRAM is reported in KB, convert to MB
                if line.contains("VRAM Total") || line.contains("Total Memory") {
                    if let Some(kb_str) = line.split_whitespace().last() {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            vram_mb = Some(kb / 1024);
                        }
                    }
                }
            }

            if !name.is_empty() {
                tracing::info!("Detected AMD GPU: {} ({} MB)", name, vram_mb.unwrap_or(0));
                return Some(GpuInfo {
                    backend: GpuBackend::Hipblas,
                    name,
                    vram_mb,
                    available: true,
                });
            }
            None
        }
        _ => None,
    }
}

/// Detect an AMD GPU by querying `hipconfig` for HIP/ROCm availability.
fn detect_amd_via_hipconfig() -> Option<GpuInfo> {
    let output = Command::new("hipconfig").output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Check if HIP is properly configured
            if stdout.contains("HIP version") || stdout.contains("ROCm") {
                // Try to get GPU name
                let name = stdout
                    .lines()
                    .find(|l| l.contains("platform") || l.contains("device"))
                    .map(|l| l.trim().to_string())
                    .unwrap_or_else(|| "AMD GPU (ROCm/HIP)".to_string());

                tracing::info!("Detected AMD GPU via HIP: {}", name);

                return Some(GpuInfo {
                    backend: GpuBackend::Hipblas,
                    name,
                    vram_mb: None,
                    available: true,
                });
            }
            None
        }
        _ => None,
    }
}

/// Detect Vulkan support by querying `vulkaninfo --summary`.
fn detect_vulkan_support() -> Option<GpuInfo> {
    let output = Command::new("vulkaninfo").args(["--summary"]).output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Parse device name from vulkaninfo output
            let device_name = stdout
                .lines()
                .find(|l| l.trim().starts_with("deviceName"))
                .map(|l| {
                    l.split('=')
                        .nth(1)
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "Unknown Vulkan Device".to_string())
                })
                .unwrap_or_else(|| "Vulkan Device".to_string());

            tracing::info!("Detected Vulkan device: {}", device_name);

            Some(GpuInfo {
                backend: GpuBackend::Vulkan,
                name: device_name,
                vram_mb: None,
                available: true,
            })
        }
        Ok(_) => {
            tracing::debug!("vulkaninfo returned error - Vulkan may not be properly configured");
            None
        }
        Err(e) => {
            tracing::debug!("vulkaninfo not available: {}", e);
            None
        }
    }
}

/// Determine the recommended GPU backend based on available hardware and compiled features.
///
/// Returns the compiled backend if a matching GPU is detected, otherwise falls back to CPU.
fn determine_recommended_backend(gpus: &[GpuInfo], compiled: GpuBackend) -> GpuBackend {
    // If we're compiled with CPU only, recommend CPU
    if compiled == GpuBackend::Cpu {
        return GpuBackend::Cpu;
    }

    // Check if the compiled backend has a matching GPU available
    let has_matching_gpu = gpus.iter().any(|gpu| gpu.backend == compiled);

    if has_matching_gpu {
        return compiled;
    }

    // If no matching GPU, but we have a GPU backend compiled, check for fallbacks
    match compiled {
        GpuBackend::Cuda => {
            // CUDA build but no NVIDIA GPU - check for Vulkan fallback
            if gpus.iter().any(|gpu| gpu.backend == GpuBackend::Vulkan) {
                tracing::warn!("CUDA build detected but no NVIDIA GPU found. Vulkan available but requires rebuild with --features vulkan");
            }
            GpuBackend::Cpu
        }
        GpuBackend::Hipblas => {
            // HIP build but no AMD GPU
            if gpus.iter().any(|gpu| gpu.backend == GpuBackend::Vulkan) {
                tracing::warn!("HIP/ROCm build detected but no AMD GPU found. Vulkan available but requires rebuild with --features vulkan");
            }
            GpuBackend::Cpu
        }
        GpuBackend::Vulkan => {
            // Vulkan build but no Vulkan support
            GpuBackend::Cpu
        }
        GpuBackend::Cpu => GpuBackend::Cpu,
    }
}

/// Check if CUDA is available on this system
pub fn is_cuda_available() -> bool {
    detect_nvidia_gpu().is_some()
}

/// Check if HIP/ROCm is available on this system
pub fn is_hip_available() -> bool {
    detect_amd_gpu().is_some()
}

/// Check if Vulkan is available on this system
pub fn is_vulkan_available() -> bool {
    detect_vulkan_support().is_some()
}

/// Microphone authorization status values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MicrophoneStatus {
    /// Microphone is available
    Granted,
    /// No microphone found or access denied
    Denied,
    /// Unable to determine status
    Unknown,
}

impl std::fmt::Display for MicrophoneStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MicrophoneStatus::Granted => write!(f, "granted"),
            MicrophoneStatus::Denied => write!(f, "denied"),
            MicrophoneStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// Check if accessibility permission is granted
///
/// On Linux, accessibility permissions are not typically required
/// for global shortcuts. X11 allows key grabbing by default.
/// On Wayland, global shortcuts require portal support.
pub fn check_accessibility_permission() -> bool {
    // On Linux, we don't have a central accessibility permission system
    // like macOS. X11 allows key grabbing by default.
    // For Wayland, the XDG Desktop Portal handles this.
    true
}

/// Open accessibility settings
///
/// On Linux, no accessibility settings need to be opened.
/// This is kept for API compatibility with macOS.
pub fn open_accessibility_settings() {
    tracing::debug!("No accessibility settings needed on Linux");
}

/// Check microphone permission status
///
/// On Linux, microphone access is managed by PulseAudio or PipeWire.
/// We check if a default audio source (microphone) is available.
pub fn check_microphone_permission() -> MicrophoneStatus {
    // Try to check PulseAudio/PipeWire for available sources
    // Use pactl to list sources

    let output = Command::new("pactl")
        .args(["list", "short", "sources"])
        .output();

    match output {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Check if there are any input sources (microphones)
                // Each line represents a source, we look for non-empty output
                let has_sources = stdout
                    .lines()
                    .any(|line| !line.trim().is_empty() && line.contains("input"));

                if has_sources {
                    tracing::debug!("Microphone available via PulseAudio/PipeWire");
                    MicrophoneStatus::Granted
                } else {
                    tracing::warn!("No microphone sources found");
                    MicrophoneStatus::Denied
                }
            } else {
                tracing::warn!("pactl command failed, trying pipewire...");
                // Try PipeWire directly if pactl fails
                check_pipewire_microphone()
            }
        }
        Err(e) => {
            tracing::warn!("pactl not available: {}, trying pipewire...", e);
            check_pipewire_microphone()
        }
    }
}

/// Check microphone via PipeWire's pw-cli
fn check_pipewire_microphone() -> MicrophoneStatus {
    let output = Command::new("pw-cli").args(["list-objects"]).output();

    match output {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Look for audio capture devices in pw-cli output
                let has_capture = stdout.contains("Audio/Source") || stdout.contains("capture");

                if has_capture {
                    tracing::debug!("Microphone available via PipeWire");
                    MicrophoneStatus::Granted
                } else {
                    tracing::warn!("No PipeWire capture devices found");
                    MicrophoneStatus::Denied
                }
            } else {
                tracing::warn!("pw-cli command failed");
                // Assume granted if we can't check - let the audio system handle it
                MicrophoneStatus::Unknown
            }
        }
        Err(e) => {
            tracing::warn!("pw-cli not available: {}", e);
            // Neither pactl nor pw-cli available - assume unknown
            // The actual audio capture will fail if there's no microphone
            MicrophoneStatus::Unknown
        }
    }
}

/// Request microphone permission
///
/// On Linux, microphone access is typically granted automatically by
/// PulseAudio/PipeWire. There's no system permission dialog like macOS.
/// This function is kept for API compatibility.
pub fn request_microphone_permission() {
    tracing::info!("Linux does not require explicit microphone permission");
    // Linux doesn't have a permission dialog system like macOS
    // Access is managed by PulseAudio/PipeWire which typically allows by default
}

/// Open system sound settings
///
/// Opens the desktop environment's sound settings panel.
pub fn open_microphone_settings() {
    // Try common desktop environment sound settings
    let commands = [
        // GNOME
        ("gnome-control-center", vec!["sound"]),
        // KDE
        ("systemsettings", vec!["kcm_pulseaudio"]),
        // Generic freedesktop
        ("xdg-open", vec!["settings://sound"]),
        // pavucontrol (PulseAudio volume control)
        ("pavucontrol", vec![]),
    ];

    for (cmd, args) in commands {
        let result = if args.is_empty() {
            Command::new(cmd).spawn()
        } else {
            Command::new(cmd).args(&args).spawn()
        };

        if result.is_ok() {
            tracing::info!("Opened sound settings via {}", cmd);
            return;
        }
    }

    tracing::warn!("Could not open sound settings - no supported settings app found");
}

/// Position of the text caret (insertion point) on screen
#[derive(Debug, Clone, Copy)]
pub struct CaretPosition {
    /// X coordinate in screen pixels
    pub x: f64,
    /// Y coordinate in screen pixels
    pub y: f64,
    /// Height of the caret/text line in pixels
    pub height: f64,
}

/// Get the position of the text caret in the currently focused application
///
/// On Linux, this requires AT-SPI2 (Assistive Technology Service Provider Interface).
/// This is a complex feature that requires:
/// 1. AT-SPI2 to be running
/// 2. The application to expose caret position via accessibility APIs
/// 3. Proper permissions to access accessibility bus
///
/// For now, this returns None as implementing AT-SPI2 support is complex.
pub fn get_caret_position() -> Option<CaretPosition> {
    // AT-SPI2 implementation would require the atspi crate
    // and proper D-Bus connection to the accessibility bus.
    // This is a significant undertaking and may not work reliably
    // across all applications and desktop environments.

    tracing::debug!("Caret position detection not implemented on Linux");
    None
}

/// Check if Input Monitoring permission is granted
///
/// On Linux, there's no equivalent to macOS Input Monitoring permission.
/// Keyboard access depends on X11/Wayland security model.
/// On X11, applications can grab keys by default.
/// On Wayland, global shortcuts require XDG Desktop Portal support.
pub fn check_input_monitoring_permission() -> bool {
    // No equivalent permission on Linux
    true
}

/// Open Input Monitoring settings
///
/// Not applicable on Linux. Logs a debug message.
pub fn open_input_monitoring_settings() {
    tracing::debug!("No input monitoring settings on Linux");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_accessibility_permission() {
        // Should always return true on Linux
        assert!(check_accessibility_permission());
    }

    #[test]
    fn test_check_microphone_permission() {
        // Just ensure it doesn't panic
        let status = check_microphone_permission();
        tracing::info!("Microphone status: {:?}", status);
    }

    #[test]
    fn test_check_input_monitoring() {
        assert!(check_input_monitoring_permission());
    }

    #[test]
    fn test_gpu_detection() {
        let result = detect_gpus();
        tracing::info!("GPU detection result: {:?}", result);

        // Verify compiled backend is detected
        #[cfg(feature = "cuda")]
        assert_eq!(result.compiled_backend, GpuBackend::Cuda);
        #[cfg(feature = "hipblas")]
        assert_eq!(result.compiled_backend, GpuBackend::Hipblas);
        #[cfg(feature = "vulkan")]
        assert_eq!(result.compiled_backend, GpuBackend::Vulkan);
        #[cfg(not(any(feature = "cuda", feature = "hipblas", feature = "vulkan")))]
        assert_eq!(result.compiled_backend, GpuBackend::Cpu);
    }

    #[test]
    fn test_gpu_backend_display() {
        assert_eq!(format!("{}", GpuBackend::Cuda), "CUDA");
        assert_eq!(format!("{}", GpuBackend::Hipblas), "HIP/ROCm");
        assert_eq!(format!("{}", GpuBackend::Vulkan), "Vulkan");
        assert_eq!(format!("{}", GpuBackend::Cpu), "CPU");
    }

    #[test]
    fn test_gpu_backend_serialization() {
        assert_eq!(
            serde_json::to_string(&GpuBackend::Cuda).unwrap(),
            "\"cuda\""
        );
        assert_eq!(
            serde_json::to_string(&GpuBackend::Hipblas).unwrap(),
            "\"hipblas\""
        );
        assert_eq!(
            serde_json::to_string(&GpuBackend::Vulkan).unwrap(),
            "\"vulkan\""
        );
        assert_eq!(serde_json::to_string(&GpuBackend::Cpu).unwrap(), "\"cpu\"");
    }

    #[test]
    fn test_determine_recommended_backend_cpu_build() {
        let gpus = vec![GpuInfo {
            backend: GpuBackend::Cuda,
            name: "NVIDIA RTX 3080".to_string(),
            vram_mb: Some(10240),
            available: true,
        }];

        // CPU build should always recommend CPU
        let result = determine_recommended_backend(&gpus, GpuBackend::Cpu);
        assert_eq!(result, GpuBackend::Cpu);
    }

    #[test]
    fn test_determine_recommended_backend_matching_gpu() {
        let gpus = vec![GpuInfo {
            backend: GpuBackend::Cuda,
            name: "NVIDIA RTX 3080".to_string(),
            vram_mb: Some(10240),
            available: true,
        }];

        // CUDA build with NVIDIA GPU should recommend CUDA
        let result = determine_recommended_backend(&gpus, GpuBackend::Cuda);
        assert_eq!(result, GpuBackend::Cuda);
    }

    #[test]
    fn test_determine_recommended_backend_no_matching_gpu() {
        let gpus = vec![GpuInfo {
            backend: GpuBackend::Vulkan,
            name: "Vulkan Device".to_string(),
            vram_mb: None,
            available: true,
        }];

        // CUDA build without NVIDIA GPU should fall back to CPU
        let result = determine_recommended_backend(&gpus, GpuBackend::Cuda);
        assert_eq!(result, GpuBackend::Cpu);
    }
}
