//! macOS-specific platform functionality

use objc2::runtime::AnyClass;
use objc2::{class, msg_send};
use objc2_foundation::NSString;
use std::process::Command;

/// Check if Input Monitoring permission is granted
///
/// Uses the IOKit framework's IOHIDCheckAccess function.
/// This is required for device_query to capture keyboard events.
pub fn check_input_monitoring_permission() -> bool {
    unsafe {
        #[link(name = "IOKit", kind = "framework")]
        extern "C" {
            fn IOHIDCheckAccess(request: u32) -> u32;
        }

        // IOHIDCheckAccess(1) checks Input Monitoring permission
        // Returns 0 if access is granted
        let result = IOHIDCheckAccess(1);
        tracing::debug!("IOHIDCheckAccess(1) returned: {}", result);
        result == 0
    }
}

/// Open System Preferences to the Input Monitoring privacy pane
pub fn open_input_monitoring_settings() {
    let result = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
        .spawn();

    match result {
        Ok(_) => tracing::info!("Opened Input Monitoring settings"),
        Err(e) => tracing::error!("Failed to open Input Monitoring settings: {}", e),
    }
}

/// Check if accessibility permission is granted
///
/// Uses the ApplicationServices framework to check if the process is trusted
/// for accessibility. This is required for global shortcuts and keystroke
/// simulation via enigo.
pub fn check_accessibility_permission() -> bool {
    unsafe {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrusted() -> bool;
        }

        AXIsProcessTrusted()
    }
}

/// Open System Preferences to the Accessibility privacy pane
pub fn open_accessibility_settings() {
    let result = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();

    match result {
        Ok(_) => tracing::info!("Opened accessibility settings"),
        Err(e) => tracing::error!("Failed to open accessibility settings: {}", e),
    }
}

/// Microphone authorization status values
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicrophoneStatus {
    /// User has not yet made a choice
    NotDetermined,
    /// Access is restricted (e.g., parental controls)
    Restricted,
    /// User explicitly denied access
    Denied,
    /// User granted access
    Authorized,
    /// Unknown status
    Unknown,
}

impl From<i64> for MicrophoneStatus {
    fn from(value: i64) -> Self {
        match value {
            0 => MicrophoneStatus::NotDetermined,
            1 => MicrophoneStatus::Restricted,
            2 => MicrophoneStatus::Denied,
            3 => MicrophoneStatus::Authorized,
            _ => MicrophoneStatus::Unknown,
        }
    }
}

impl std::fmt::Display for MicrophoneStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MicrophoneStatus::NotDetermined => write!(f, "not_determined"),
            MicrophoneStatus::Restricted => write!(f, "restricted"),
            MicrophoneStatus::Denied => write!(f, "denied"),
            MicrophoneStatus::Authorized => write!(f, "granted"),
            MicrophoneStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// Check microphone permission status
///
/// Uses AVFoundation's AVCaptureDevice to check authorization status for audio.
/// Returns the detailed status.
pub fn check_microphone_permission() -> MicrophoneStatus {
    unsafe {
        // Link AVFoundation framework
        #[link(name = "AVFoundation", kind = "framework")]
        extern "C" {}

        // Get AVCaptureDevice class
        let cls: &AnyClass = class!(AVCaptureDevice);

        // Create NSString for "soun" (AVMediaTypeAudio)
        let media_type = NSString::from_str("soun");

        // Call authorizationStatusForMediaType:
        // Returns: 0=NotDetermined, 1=Restricted, 2=Denied, 3=Authorized
        let status: i64 = msg_send![cls, authorizationStatusForMediaType: &*media_type];

        tracing::debug!("Microphone authorization status: {}", status);
        MicrophoneStatus::from(status)
    }
}

/// Request microphone permission
///
/// Triggers the system permission dialog for microphone access.
/// Note: This returns immediately; the actual user response is handled asynchronously.
pub fn request_microphone_permission() {
    unsafe {
        #[link(name = "AVFoundation", kind = "framework")]
        extern "C" {}

        let cls: &AnyClass = class!(AVCaptureDevice);
        let media_type = NSString::from_str("soun");

        // Call requestAccessForMediaType:completionHandler:
        // We pass a nil completion handler since we don't need the callback
        let nil: *const std::ffi::c_void = std::ptr::null();
        let _: () = msg_send![
            cls,
            requestAccessForMediaType: &*media_type,
            completionHandler: nil
        ];

        tracing::info!("Requested microphone permission");
    }
}

/// Open System Preferences to the Microphone privacy pane
pub fn open_microphone_settings() {
    let result = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        .spawn();

    match result {
        Ok(_) => tracing::info!("Opened microphone settings"),
        Err(e) => tracing::error!("Failed to open microphone settings: {}", e),
    }
}

/// Verify that accessibility permission is functionally working, not just granted.
///
/// `AXIsProcessTrusted()` can return `true` even when the TCC database entry is
/// stale (e.g., after reinstall with a different code signature). This function
/// goes further by actually attempting an Accessibility API call to confirm the
/// permission is live.
///
/// Returns `true` if accessibility is genuinely working.
/// Returns `false` if not granted, or if granted but stale/non-functional.
pub fn verify_accessibility_functional() -> bool {
    if !check_accessibility_permission() {
        return false;
    }

    // Actually attempt an AX API call to verify the permission is live
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    unsafe {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXUIElementCreateSystemWide() -> *mut std::ffi::c_void;
            fn AXUIElementCopyAttributeValue(
                element: *mut std::ffi::c_void,
                attribute: *const std::ffi::c_void,
                value: *mut *mut std::ffi::c_void,
            ) -> i32;
            fn CFRelease(cf: *const std::ffi::c_void);
        }

        // kAXErrorCannotComplete — returned when permission is stale
        const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;

        let system_wide = AXUIElementCreateSystemWide();
        if system_wide.is_null() {
            return false;
        }

        let attr = CFString::new("AXFocusedApplication");
        let mut value: *mut std::ffi::c_void = std::ptr::null_mut();
        let result = AXUIElementCopyAttributeValue(
            system_wide,
            attr.as_concrete_TypeRef() as *const _,
            &mut value,
        );

        if !value.is_null() {
            CFRelease(value as *const _);
        }
        CFRelease(system_wide as *const _);

        // Result 0 = success (got a focused app).
        // kAXErrorNoValue (-25212) = no focused app, but the API is responding — permission works.
        // kAXErrorCannotComplete (-25204) = permission is stale/broken.
        if result == AX_ERROR_CANNOT_COMPLETE {
            tracing::warn!(
                "Accessibility permission appears granted but AX API returned \
                 kAXErrorCannotComplete — TCC entry is likely stale"
            );
            return false;
        }

        true
    }
}

/// Reset TCC (Transparency, Consent, and Control) permission entries for Thoth.
///
/// Uses `tccutil reset` via `osascript` to prompt for administrator privileges,
/// which is required for system-level permissions (Accessibility, Input Monitoring).
///
/// Valid service names: "Accessibility", "ListenEvent", "Microphone", "All"
pub fn reset_tcc_permissions(services: &[String]) -> Result<String, String> {
    use std::process::Command;

    if services.is_empty() {
        return Err("No services specified to reset.".to_string());
    }

    const BUNDLE_ID: &str = "com.poodle64.thoth";

    // Allowlist of valid TCC service names to prevent command injection
    const VALID_SERVICES: &[&str] = &[
        "Accessibility",
        "ListenEvent",
        "Microphone",
        "Camera",
        "ScreenCapture",
        "All",
    ];

    for service in services {
        if !VALID_SERVICES.contains(&service.as_str()) {
            return Err(format!("Invalid TCC service name: {}", service));
        }
    }

    let commands: Vec<String> = services
        .iter()
        .map(|s| format!("tccutil reset {} {}", s, BUNDLE_ID))
        .collect();
    let script = commands.join(" && ");

    tracing::info!("Resetting TCC permissions: {}", script);

    let output = Command::new("osascript")
        .arg("-e")
        .arg(format!(
            "do shell script \"{}\" with administrator privileges",
            script
        ))
        .output()
        .map_err(|e| format!("Failed to execute tccutil: {}", e))?;

    if output.status.success() {
        tracing::info!(
            "TCC permissions reset successfully for services: {:?}",
            services
        );
        Ok("Permissions reset successfully. Please re-grant them in System Settings.".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("TCC reset failed: {}", stderr);
        Err(format!("Failed to reset permissions: {}", stderr))
    }
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
/// Uses macOS Accessibility APIs to find the focused text element and get
/// the bounds of the text insertion point. This is used to position the
/// recording indicator near where text will be inserted (like macOS dictation).
///
/// Returns None if:
/// - No text field is focused
/// - The focused element doesn't have a text insertion point
/// - Accessibility permission is not granted
pub fn get_caret_position() -> Option<CaretPosition> {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use core_graphics::geometry::CGRect;

    unsafe {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXUIElementCreateSystemWide() -> *mut std::ffi::c_void;
            fn AXUIElementCopyAttributeValue(
                element: *mut std::ffi::c_void,
                attribute: *const std::ffi::c_void,
                value: *mut *mut std::ffi::c_void,
            ) -> i32;
            fn AXValueGetValue(
                value: *mut std::ffi::c_void,
                value_type: i32,
                value_ptr: *mut std::ffi::c_void,
            ) -> bool;
            fn CFRelease(cf: *const std::ffi::c_void);
        }

        // AXValue types
        const AX_VALUE_TYPE_CGRECT: i32 = 3;

        // Create system-wide accessibility element
        let system_wide = AXUIElementCreateSystemWide();
        if system_wide.is_null() {
            tracing::debug!("Failed to create system-wide AXUIElement");
            return None;
        }

        // Get the focused application
        let focused_app_attr = CFString::new("AXFocusedApplication");
        let mut focused_app: *mut std::ffi::c_void = std::ptr::null_mut();
        let result = AXUIElementCopyAttributeValue(
            system_wide,
            focused_app_attr.as_concrete_TypeRef() as *const _,
            &mut focused_app,
        );

        if result != 0 || focused_app.is_null() {
            CFRelease(system_wide as *const _);
            tracing::debug!("No focused application");
            return None;
        }

        // Get the focused UI element within the application
        let focused_element_attr = CFString::new("AXFocusedUIElement");
        let mut focused_element: *mut std::ffi::c_void = std::ptr::null_mut();
        let result = AXUIElementCopyAttributeValue(
            focused_app,
            focused_element_attr.as_concrete_TypeRef() as *const _,
            &mut focused_element,
        );

        CFRelease(focused_app as *const _);
        CFRelease(system_wide as *const _);

        if result != 0 || focused_element.is_null() {
            tracing::debug!("No focused UI element");
            return None;
        }

        // Try to get the insertion point bounds directly
        // Note: AXBoundsForRange could be used for more precise positioning but
        // requires creating an AXValueRef for the range. AXInsertionPointBounds
        // is simpler and works for our use case.
        // Some apps expose AXSelectedTextMarkerRange or similar
        let insertion_point_attr = CFString::new("AXInsertionPointBounds");
        let mut bounds_value: *mut std::ffi::c_void = std::ptr::null_mut();
        let result = AXUIElementCopyAttributeValue(
            focused_element,
            insertion_point_attr.as_concrete_TypeRef() as *const _,
            &mut bounds_value,
        );

        if result == 0 && !bounds_value.is_null() {
            // Got insertion point bounds directly
            let mut rect = CGRect::default();
            if AXValueGetValue(
                bounds_value,
                AX_VALUE_TYPE_CGRECT,
                &mut rect as *mut CGRect as *mut std::ffi::c_void,
            ) {
                CFRelease(bounds_value as *const _);
                CFRelease(focused_element as *const _);

                tracing::debug!(
                    "Got caret position from AXInsertionPointBounds: ({}, {})",
                    rect.origin.x,
                    rect.origin.y
                );

                return Some(CaretPosition {
                    x: rect.origin.x,
                    y: rect.origin.y,
                    height: rect.size.height.max(20.0), // Minimum height
                });
            }
            CFRelease(bounds_value as *const _);
        }

        // Fallback: try to get the position of the focused element itself
        let position_attr = CFString::new("AXPosition");
        let size_attr = CFString::new("AXSize");

        let mut position_value: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut size_value: *mut std::ffi::c_void = std::ptr::null_mut();

        let pos_result = AXUIElementCopyAttributeValue(
            focused_element,
            position_attr.as_concrete_TypeRef() as *const _,
            &mut position_value,
        );

        let size_result = AXUIElementCopyAttributeValue(
            focused_element,
            size_attr.as_concrete_TypeRef() as *const _,
            &mut size_value,
        );

        CFRelease(focused_element as *const _);

        if pos_result == 0 && !position_value.is_null() {
            use core_graphics::geometry::CGPoint;
            use core_graphics::geometry::CGSize;

            let mut point = CGPoint::default();
            const AX_VALUE_TYPE_CGPOINT: i32 = 1;

            if AXValueGetValue(
                position_value,
                AX_VALUE_TYPE_CGPOINT,
                &mut point as *mut CGPoint as *mut std::ffi::c_void,
            ) {
                let mut height = 20.0; // Default height

                if size_result == 0 && !size_value.is_null() {
                    let mut size = CGSize::default();
                    const AX_VALUE_TYPE_CGSIZE: i32 = 2;
                    if AXValueGetValue(
                        size_value,
                        AX_VALUE_TYPE_CGSIZE,
                        &mut size as *mut CGSize as *mut std::ffi::c_void,
                    ) {
                        height = size.height;
                    }
                    CFRelease(size_value as *const _);
                }

                CFRelease(position_value as *const _);

                tracing::debug!(
                    "Got focused element position: ({}, {}), height={}",
                    point.x,
                    point.y,
                    height
                );

                // Position at the right edge of the element (where text cursor typically is)
                return Some(CaretPosition {
                    x: point.x,
                    y: point.y,
                    height,
                });
            }
            CFRelease(position_value as *const _);
        }

        if !size_value.is_null() {
            CFRelease(size_value as *const _);
        }

        tracing::debug!("Could not determine caret position");
        None
    }
}

/// Check if the screen is locked or the screensaver is active.
///
/// Uses `CGSessionCopyCurrentDictionary()` from ApplicationServices to query
/// the `CGSSessionScreenIsLocked` key. Returns `true` when the lock screen
/// or screensaver is showing, so global shortcuts can be suppressed.
pub fn is_screen_locked() -> bool {
    unsafe {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn CGSessionCopyCurrentDictionary() -> *const std::ffi::c_void;
        }

        let dict = CGSessionCopyCurrentDictionary();
        if dict.is_null() {
            // Can't determine — assume not locked
            return false;
        }

        use core_foundation::base::TCFType;
        use core_foundation::boolean::CFBoolean;
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::string::CFString;

        let cf_dict = CFDictionary::<CFString, CFBoolean>::wrap_under_create_rule(dict as *const _);

        let key = CFString::new("CGSSessionScreenIsLocked");

        let locked = cf_dict
            .find(key)
            .is_some_and(|val| bool::from((*val).clone()));

        if locked {
            tracing::debug!("Screen is locked — suppressing shortcut");
        }

        locked
    }
}

/// Register a listener for system wake-from-sleep events.
///
/// When the Mac wakes up, the CoreML/ONNX compilation cache may have been
/// evicted. We re-warm the transcription model in the background so the
/// first recording after wake isn't penalised.
pub fn register_wake_observer() {
    std::thread::spawn(|| {
        let mut last_check = std::time::Instant::now();

        loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let now = std::time::Instant::now();
            let elapsed = now.duration_since(last_check);

            // If more than 30 seconds have passed in what should be ~5 seconds,
            // the system was asleep.
            if elapsed > std::time::Duration::from_secs(30) {
                tracing::info!(
                    "Detected wake from sleep ({:.0}s gap), re-warming transcription model",
                    elapsed.as_secs_f64()
                );
                // Brief delay to let the system stabilise after wake
                std::thread::sleep(std::time::Duration::from_secs(3));
                crate::transcription::warmup_transcription();
                // Invalidate mouse tracker's cached monitor bounds
                crate::mouse_tracker::notify_wake();
            }

            last_check = now;
        }
    });

    tracing::info!("Registered wake-from-sleep observer for model re-warming");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_accessibility() {
        // This test just ensures the function doesn't panic
        let _result = check_accessibility_permission();
    }

    #[test]
    fn test_check_microphone() {
        // This test just ensures the function doesn't panic
        let _result = check_microphone_permission();
    }

    #[test]
    fn test_get_caret_position() {
        // This test just ensures the function doesn't panic
        // It will return None if run without a focused text field
        let _result = get_caret_position();
    }
}
