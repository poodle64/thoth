//! Audio device enumeration using cpal

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::DeviceId;
use serde::Serialize;
use std::str::FromStr;

/// Represents an audio input device
#[derive(Debug, Clone, Serialize)]
pub struct AudioDevice {
    /// Unique identifier for the device (stable across restarts)
    pub id: String,
    /// Human-readable device name
    pub name: String,
    /// Whether this is the system default input device
    pub is_default: bool,
}

/// Get the display name for a device
///
/// Uses `description()` as the primary method (cpal 0.17+), with `name()` as fallback
/// for edge cases where description isn't available.
pub fn get_device_display_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|desc| desc.name().to_string())
        .unwrap_or_else(|_| {
            // Fallback to deprecated name() only when description() fails
            #[allow(deprecated)]
            device.name().unwrap_or_else(|_| "Unknown".to_string())
        })
}

/// List all available audio input devices
///
/// Returns a vector of available input devices with their names and
/// whether they are the default device.
///
/// Uses cpal's DeviceId for stable identification across restarts.
pub fn list_input_devices() -> Vec<AudioDevice> {
    let host = cpal::default_host();

    // Log host information for debugging Linux audio issues
    tracing::info!("CPAL host: {}", host.id().name());

    // Get default device ID for comparison
    let default_device = host.default_input_device();
    let default_device_id = default_device
        .as_ref()
        .and_then(|d| d.id().ok())
        .map(|id| id.to_string());

    if let Some(ref device) = default_device {
        let name = get_device_display_name(device);
        if let Ok(config) = device.default_input_config() {
            tracing::info!(
                "Default input device: '{}', {}Hz, {}ch, format={:?}",
                name,
                config.sample_rate(),
                config.channels(),
                config.sample_format()
            );
        }
    } else {
        tracing::warn!("No default input device found!");
    }

    // Enumerate all input devices
    let devices: Vec<AudioDevice> = host
        .input_devices()
        .map(|device_iter| {
            device_iter
                .filter_map(|device| {
                    // Use id() for stable identification (persists across restarts)
                    let device_id = device.id().ok()?.to_string();
                    let device_name = get_device_display_name(&device);

                    // Log each device for debugging
                    if let Ok(config) = device.default_input_config() {
                        tracing::debug!(
                            "Found input device: '{}' (id: {}), {}Hz, {}ch",
                            device_name,
                            device_id,
                            config.sample_rate(),
                            config.channels()
                        );
                    }

                    Some(AudioDevice {
                        id: device_id.clone(),
                        name: device_name,
                        is_default: Some(&device_id) == default_device_id.as_ref(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    tracing::info!("Found {} input devices", devices.len());
    devices
}

/// Get the default input device
pub fn get_default_input_device() -> Option<cpal::Device> {
    let host = cpal::default_host();
    host.default_input_device()
}

/// Find an input device by its stable ID
///
/// Uses cpal's DeviceId for reliable device lookup across restarts.
pub fn find_input_device_by_id(id_str: &str) -> Option<cpal::Device> {
    let host = cpal::default_host();

    // Parse the stored ID string back to a DeviceId
    let device_id = DeviceId::from_str(id_str).ok()?;

    // Use cpal's device_by_id for efficient lookup
    host.device_by_id(&device_id)
}

/// Get the input device to use for recording, based on config
///
/// If a device ID is configured and found, uses that device.
/// Otherwise falls back to the system default.
pub fn get_recording_device(device_id: Option<&str>) -> Option<cpal::Device> {
    tracing::info!("get_recording_device called with device_id: {:?}", device_id);

    if let Some(id) = device_id {
        if let Some(device) = find_input_device_by_id(id) {
            let name = get_device_display_name(&device);
            tracing::info!("Using configured audio device: {}", name);
            return Some(device);
        }

        // Log available devices so it's easy to diagnose why the configured one wasn't found
        let available = list_input_devices();
        let device_list: Vec<String> = available
            .iter()
            .map(|d| format!("{} (id: {})", d.name, d.id))
            .collect();
        tracing::warn!(
            "Configured audio device '{}' not found. Available devices: [{}]. Falling back to default.",
            id,
            device_list.join(", ")
        );
    }

    let device = get_default_input_device();
    if let Some(ref d) = device {
        let name = get_device_display_name(d);
        if let Ok(config) = d.default_input_config() {
            tracing::info!(
                "Using default audio device: '{}', {}Hz, {}ch, format={:?}",
                name,
                config.sample_rate(),
                config.channels(),
                config.sample_format()
            );
        } else {
            tracing::info!("Using default audio device: '{}' (could not get config)", name);
        }
    } else {
        tracing::error!("No default input device available!");
    }
    device
}

/// Tauri command to list audio devices
#[tauri::command]
pub fn list_audio_devices() -> Vec<AudioDevice> {
    let devices = list_input_devices();
    tracing::debug!("Found {} audio input devices", devices.len());
    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_input_devices() {
        let devices = list_input_devices();
        // Should return at least an empty list without panicking
        println!("Found {} devices", devices.len());
        for device in &devices {
            println!(
                "  - {} (id: {}, default: {})",
                device.name, device.id, device.is_default
            );
        }
    }

    #[test]
    fn test_get_default_device() {
        // Should not panic even if no device available
        let _device = get_default_input_device();
    }

    #[test]
    fn test_device_id_stable_format() {
        // Verify the ID format is parseable back to DeviceId
        let devices = list_input_devices();
        for device in &devices {
            let parsed = DeviceId::from_str(&device.id);
            assert!(
                parsed.is_ok(),
                "Device ID '{}' should be parseable as DeviceId",
                device.id
            );
        }
    }
}
