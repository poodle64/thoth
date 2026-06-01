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

        // Mirrors Apple's private kIOHIDRequestTypeListenEvent / kIOHIDAccessTypeGranted
        // (undocumented IOHIDCheckAccess API; values confirmed via IOKit private headers).
        const IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
        const IOHID_ACCESS_TYPE_GRANTED: u32 = 0;

        let result = IOHIDCheckAccess(IOHID_REQUEST_TYPE_LISTEN_EVENT);
        tracing::debug!("IOHIDCheckAccess(LISTEN_EVENT) returned: {}", result);
        result == IOHID_ACCESS_TYPE_GRANTED
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
        #[link(name = "AVFoundation", kind = "framework")]
        extern "C" {}

        let cls: &AnyClass = class!(AVCaptureDevice);
        let media_type = NSString::from_str("soun");

        // authorizationStatusForMediaType: returns 0..3 — the NSString must stay
        // alive for the duration of the message send but is not retained by the
        // callee, so binding it above is sufficient.
        let status: i64 = msg_send![cls, authorizationStatusForMediaType: &*media_type];

        tracing::debug!("Microphone authorization status: {}", status);
        MicrophoneStatus::from(status)
    }
}

/// Request microphone permission
///
/// Triggers the system permission dialog for microphone access.
/// The completion handler emits a `permission-changed` event so the
/// frontend can re-check immediately when the user responds.
pub fn request_microphone_permission(app: tauri::AppHandle) {
    unsafe {
        #[link(name = "AVFoundation", kind = "framework")]
        extern "C" {}

        let cls: &AnyClass = class!(AVCaptureDevice);
        let media_type = NSString::from_str("soun");

        // requestAccessForMediaType:completionHandler: requires a proper ObjC
        // block, not a null pointer. The completion handler fires on a background
        // thread when the user responds to the system dialog.
        let handler: block2::RcBlock<dyn Fn(objc2::runtime::Bool)> =
            block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
                tracing::info!("Microphone permission response: {}", granted.as_bool());
                // Notify frontend so it can re-check all permissions instantly
                use tauri::Emitter;
                let _ = app.emit("permission-changed", "microphone");
            });

        let _: () = msg_send![
            cls,
            requestAccessForMediaType: &*media_type,
            completionHandler: &*handler
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

/// Reset the permissions that an update is likely to have invalidated, then
/// point the user at System Settings to re-grant them.
///
/// macOS keys TCC grants to the app's code-signing identity; a self-built or
/// non-notarised Thoth gets a fresh identity on each build, so after an update
/// the old grants silently stop working ("appears granted but is stale"). Rather
/// than have the user diagnose that, we reset the three permissions Thoth uses
/// the moment we detect a version change, so they re-grant once from a clean
/// slate. Best-effort: a failure (e.g. the user cancels the admin prompt) is
/// logged and returned, not fatal.
pub fn reset_permissions_after_update() -> Result<String, String> {
    let services = vec![
        "Accessibility".to_string(),
        "ListenEvent".to_string(),
        "Microphone".to_string(),
    ];
    reset_tcc_permissions(&services)
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

/// Milliseconds since the Unix epoch of the most recent wake event we acted on.
/// Used to debounce the two notifications (DidWake + ScreensDidWake) that both
/// fire on a full system wake. Zero means no wake has fired yet.
static LAST_WAKE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Run the wake recovery sequence on a background thread.
///
/// Kept separate so the NSWorkspace notification block stays cheap
/// (grab atomic, maybe spawn, return) and this body can be called
/// identically for both DidWake and ScreensDidWake.
fn handle_wake(source: &str) {
    use std::sync::atomic::Ordering;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    // Milliseconds since epoch — fits in u64 for centuries.
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64;

    // Debounce: both DidWake and ScreensDidWake fire on a full system wake.
    // If we handled a wake within the last second, skip this duplicate.
    let prev = LAST_WAKE_MS.load(Ordering::Relaxed);
    if prev > 0 && now_ms.saturating_sub(prev) < 1000 {
        tracing::debug!("Wake notification '{}' suppressed (debounce)", source);
        return;
    }
    LAST_WAKE_MS.store(now_ms, Ordering::Relaxed);

    tracing::info!("System wake detected via '{}'", source);

    std::thread::spawn(|| {
        // Invalidate the mouse tracker's caches FIRST (a cheap atomic)
        // so the indicator can recover immediately, without waiting on
        // the multi-second model re-warm below.
        crate::mouse_tracker::notify_wake();
        // A held cpal stream handle goes stale across sleep; drop it so
        // the next recording opens a fresh device handle.
        crate::audio::cool_down_recording();
        // Then re-warm the transcription model: the CoreML/ONNX compile
        // cache may have been evicted across sleep, so the first
        // recording after wake would otherwise be penalised.
        std::thread::sleep(std::time::Duration::from_secs(3));
        crate::transcription::warmup_transcription();
    });
}

/// Register NSWorkspace observers for system wake and display-wake events.
///
/// Uses `NSWorkspaceDidWakeNotification` (full wake from sleep) and
/// `NSWorkspaceScreensDidWakeNotification` (display wake, e.g. lid open or
/// HDMI reconnect). The poll-based heuristic missed display-only wakes and
/// short lid-close events; real OS notifications are exact.
///
/// Observer tokens are leaked for process lifetime — the returned object is
/// not Send, so it cannot live in a static Mutex; leaking is the correct
/// pattern for a once-registered, never-removed observer.
pub fn register_wake_observer() {
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{
        NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceScreensDidWakeNotification,
    };
    use objc2_foundation::NSOperationQueue;
    use std::ptr::NonNull;

    unsafe {
        let workspace = NSWorkspace::sharedWorkspace();
        let center = workspace.notificationCenter();
        let queue = NSOperationQueue::mainQueue();

        // Build one block per notification name. Each block is cheap: debounce
        // check + optional thread spawn. The `move` captures nothing heap-heavy.
        let did_wake_block: block2::RcBlock<dyn Fn(NonNull<objc2_foundation::NSNotification>)> =
            block2::RcBlock::new(move |_notif: NonNull<objc2_foundation::NSNotification>| {
                handle_wake("NSWorkspaceDidWakeNotification");
            });

        let screens_did_wake_block: block2::RcBlock<
            dyn Fn(NonNull<objc2_foundation::NSNotification>),
        > = block2::RcBlock::new(move |_notif: NonNull<objc2_foundation::NSNotification>| {
            handle_wake("NSWorkspaceScreensDidWakeNotification");
        });

        let token_did_wake = center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidWakeNotification),
            None::<&AnyObject>,
            Some(&*queue),
            &did_wake_block,
        );

        let token_screens_wake = center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceScreensDidWakeNotification),
            None::<&AnyObject>,
            Some(&*queue),
            &screens_did_wake_block,
        );

        // Leak the tokens for process lifetime — removing these observers is
        // not needed (the app exits, taking the notification centre with it).
        Box::leak(Box::new(token_did_wake));
        Box::leak(Box::new(token_screens_wake));
    }

    tracing::info!("Registered NSWorkspace wake observers (DidWake + ScreensDidWake)");
}

// ---------------------------------------------------------------------------
// Bluetooth transport-type detection
// ---------------------------------------------------------------------------

/// CoreAudio transport-type constants (verified in objc2-core-audio 0.3.2
/// AudioHardware.rs — all values confirmed by grepping the generated source).
mod transport {
    pub const BUILT_IN: u32 = 0x626c746e; // kAudioDeviceTransportTypeBuiltIn
    pub const BLUETOOTH: u32 = 0x626c7565; // kAudioDeviceTransportTypeBluetooth
    pub const BLUETOOTH_LE: u32 = 0x626c6561; // kAudioDeviceTransportTypeBluetoothLE
}

/// Read a single `u32` CoreAudio property from a device object.
///
/// Returns `None` on any FFI error so callers degrade gracefully.
///
/// # Safety
/// Only safe to call after verifying `property_size` matches the u32 layout via
/// `AudioObjectGetPropertyDataSize`. This wrapper handles both size-check and read.
fn read_audio_property_u32(device_id: u32, selector: u32, scope: u32) -> Option<u32> {
    use core::ffi::c_void;
    use core::ptr::NonNull;
    use objc2_core_audio::{
        kAudioHardwareNoError, kAudioObjectPropertyElementMain, AudioObjectGetPropertyData,
        AudioObjectGetPropertyDataSize, AudioObjectPropertyAddress,
    };

    let address = AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut address = address;

    // SAFETY: address is a valid stack allocation; qualifier args are null/0 (none required).
    let mut data_size: u32 = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            device_id,
            NonNull::new_unchecked(&mut address),
            0,
            std::ptr::null(),
            NonNull::new_unchecked(&mut data_size),
        )
    };
    if status != kAudioHardwareNoError {
        return None;
    }
    if data_size as usize != std::mem::size_of::<u32>() {
        return None;
    }

    let mut value: u32 = 0;
    // SAFETY: value is a valid u32 on the stack; size confirmed above.
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            NonNull::new_unchecked(&mut address),
            0,
            std::ptr::null(),
            NonNull::new_unchecked(&mut data_size),
            NonNull::new_unchecked(&mut value as *mut u32 as *mut c_void),
        )
    };
    if status != kAudioHardwareNoError {
        None
    } else {
        Some(value)
    }
}

/// Read the transport type of a CoreAudio device object (by numeric ID).
fn device_transport_type(device_id: u32) -> Option<u32> {
    use objc2_core_audio::{kAudioDevicePropertyTransportType, kAudioObjectPropertyScopeGlobal};
    read_audio_property_u32(
        device_id,
        kAudioDevicePropertyTransportType,
        kAudioObjectPropertyScopeGlobal,
    )
}

/// Query the default input device's transport type and return `true` if it is
/// Bluetooth (classic or LE). Returns `false` on any FFI failure so callers
/// degrade gracefully — an FFI error never blocks recording.
pub fn default_input_transport_is_bluetooth() -> bool {
    use core::ffi::c_void;
    use core::ptr::NonNull;
    use objc2_core_audio::{
        kAudioHardwareNoError, kAudioHardwarePropertyDefaultInputDevice,
        kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
        AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectPropertyAddress,
    };

    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultInputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut address = address;
    let sys_obj = kAudioObjectSystemObject as u32;

    let mut data_size: u32 = 0;
    // SAFETY: address and data_size are valid stack allocations.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            sys_obj,
            NonNull::new_unchecked(&mut address),
            0,
            std::ptr::null(),
            NonNull::new_unchecked(&mut data_size),
        )
    };
    if status != kAudioHardwareNoError {
        tracing::warn!("CoreAudio: AudioObjectGetPropertyDataSize for default input failed ({}); assuming non-Bluetooth", status);
        return false;
    }

    let mut default_device_id: u32 = 0;
    // SAFETY: default_device_id is a valid u32 stack allocation; size confirmed above.
    let status = unsafe {
        AudioObjectGetPropertyData(
            sys_obj,
            NonNull::new_unchecked(&mut address),
            0,
            std::ptr::null(),
            NonNull::new_unchecked(&mut data_size),
            NonNull::new_unchecked(&mut default_device_id as *mut u32 as *mut c_void),
        )
    };
    if status != kAudioHardwareNoError {
        tracing::warn!("CoreAudio: AudioObjectGetPropertyData for default input failed ({}); assuming non-Bluetooth", status);
        return false;
    }

    let transport = match device_transport_type(default_device_id) {
        Some(t) => t,
        None => {
            tracing::warn!("CoreAudio: could not read transport type of default input device; assuming non-Bluetooth");
            return false;
        }
    };

    let is_bt = transport == transport::BLUETOOTH || transport == transport::BLUETOOTH_LE;
    if is_bt {
        tracing::debug!(
            "Default input device (id={}) has Bluetooth transport type (0x{:x})",
            default_device_id,
            transport
        );
    }
    is_bt
}

/// Enumerate CoreAudio devices and return the display name of the first
/// Built-in input device found.
///
/// Approach: enumerate all devices via `kAudioHardwarePropertyDevices`, check each
/// for `kAudioDeviceTransportTypeBuiltIn`, then read its `kAudioObjectPropertyName`
/// as a CFStringRef. The CoreAudio name is the system name (e.g. "MacBook Pro
/// Microphone"), which matches what cpal returns via `device.name()`.
///
/// Returns `None` if no built-in input device is found or if all FFI calls fail.
pub fn builtin_input_device_name() -> Option<String> {
    use core::ffi::c_void;
    use core::ptr::NonNull;
    use objc2_core_audio::{
        kAudioDevicePropertyStreamConfiguration, kAudioObjectPropertyName,
        kAudioObjectPropertyScopeInput,
    };
    use objc2_core_audio::{
        kAudioHardwareNoError, kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, AudioObjectGetPropertyData,
        AudioObjectGetPropertyDataSize, AudioObjectPropertyAddress,
    };
    use objc2_core_foundation::{CFRetained, CFString, CFStringBuiltInEncodings};

    let sys_obj = kAudioObjectSystemObject as u32;

    // --- Step 1: get the list of all CoreAudio devices. ---
    let devices_address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut devices_address = devices_address;

    let mut devices_size: u32 = 0;
    // SAFETY: devices_address and devices_size are valid stack allocations.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            sys_obj,
            NonNull::new_unchecked(&mut devices_address),
            0,
            std::ptr::null(),
            NonNull::new_unchecked(&mut devices_size),
        )
    };
    if status != kAudioHardwareNoError {
        tracing::warn!("CoreAudio: could not get device list size ({})", status);
        return None;
    }

    let device_count = devices_size as usize / std::mem::size_of::<u32>();
    if device_count == 0 {
        return None;
    }

    let mut device_ids: Vec<u32> = vec![0u32; device_count];
    // SAFETY: device_ids is a valid Vec<u32> with the confirmed byte size.
    let status = unsafe {
        AudioObjectGetPropertyData(
            sys_obj,
            NonNull::new_unchecked(&mut devices_address),
            0,
            std::ptr::null(),
            NonNull::new_unchecked(&mut devices_size),
            NonNull::new_unchecked(device_ids.as_mut_ptr() as *mut c_void),
        )
    };
    if status != kAudioHardwareNoError {
        tracing::warn!("CoreAudio: could not read device list ({})", status);
        return None;
    }

    // --- Step 2: find the first Built-in device with input channels. ---
    for &dev_id in &device_ids {
        // Check transport type.
        let transport = match device_transport_type(dev_id) {
            Some(t) => t,
            None => continue,
        };
        if transport != transport::BUILT_IN {
            continue;
        }

        // Confirm the device has at least one input stream by checking its
        // stream configuration on the input scope. A Built-in device may be
        // output-only (e.g. a virtual aggregate output device).
        let stream_cfg_addr = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: kAudioObjectPropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut stream_cfg_addr = stream_cfg_addr;

        let mut stream_size: u32 = 0;
        // SAFETY: stream_cfg_addr and stream_size are valid stack allocations.
        let stream_status = unsafe {
            AudioObjectGetPropertyDataSize(
                dev_id,
                NonNull::new_unchecked(&mut stream_cfg_addr),
                0,
                std::ptr::null(),
                NonNull::new_unchecked(&mut stream_size),
            )
        };
        if stream_status != kAudioHardwareNoError || stream_size == 0 {
            // No input streams on this built-in device — skip.
            continue;
        }

        // --- Step 3: read the device name as a CFStringRef. ---
        // kAudioObjectPropertyName returns a CFStringRef (a retained CF object).
        // The data size is sizeof(CFStringRef) = pointer size.
        let name_addr = AudioObjectPropertyAddress {
            mSelector: kAudioObjectPropertyName,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut name_addr = name_addr;

        let mut name_size: u32 = std::mem::size_of::<*const c_void>() as u32;
        let mut cf_str_ptr: *const CFString = std::ptr::null();

        // SAFETY: cf_str_ptr is a valid pointer-sized allocation; name_addr is a valid stack address.
        let name_status = unsafe {
            AudioObjectGetPropertyData(
                dev_id,
                NonNull::new_unchecked(&mut name_addr),
                0,
                std::ptr::null(),
                NonNull::new_unchecked(&mut name_size),
                NonNull::new_unchecked(&mut cf_str_ptr as *mut *const CFString as *mut c_void),
            )
        };
        if name_status != kAudioHardwareNoError || cf_str_ptr.is_null() {
            tracing::warn!(
                "CoreAudio: could not read name for built-in device id={} ({})",
                dev_id,
                name_status
            );
            continue;
        }

        // Wrap in CFRetained so the CFString is released when it goes out of scope.
        // SAFETY: cf_str_ptr is a non-null, just-retained CFStringRef from CoreAudio.
        let cf_string: CFRetained<CFString> =
            unsafe { CFRetained::from_raw(NonNull::new_unchecked(cf_str_ptr as *mut CFString)) };

        // Extract as a Rust String using CFStringGetCString with UTF-8 encoding.
        let len = cf_string.length();
        // Allow 4 bytes per code unit (worst-case UTF-8) plus a null terminator.
        let buf_len = (len as usize) * 4 + 1;
        let mut buf: Vec<u8> = vec![0u8; buf_len];

        // SAFETY: buf is a valid allocation of buf_len bytes; EncodingUTF8 is a valid encoding.
        let ok = unsafe {
            cf_string.c_string(
                buf.as_mut_ptr() as *mut i8,
                buf_len as isize,
                CFStringBuiltInEncodings::EncodingUTF8.0,
            )
        };

        if ok {
            // Find the null terminator and convert.
            let c_str = std::ffi::CStr::from_bytes_until_nul(&buf).ok()?;
            let name = c_str.to_str().ok()?.to_string();
            tracing::debug!(
                "CoreAudio: found built-in input device id={} name='{}'",
                dev_id,
                name
            );
            return Some(name);
        } else {
            tracing::warn!(
                "CoreAudio: CFStringGetCString failed for built-in device id={}",
                dev_id
            );
        }
    }

    tracing::debug!("CoreAudio: no built-in input device found");
    None
}

/// Read a CoreAudio device's display name (`kAudioObjectPropertyName`) as a
/// Rust `String`. Returns `None` on any FFI failure.
#[cfg(target_os = "macos")]
fn read_device_name(dev_id: u32) -> Option<String> {
    use core::ffi::c_void;
    use core::ptr::NonNull;
    use objc2_core_audio::{
        kAudioHardwareNoError, kAudioObjectPropertyElementMain, kAudioObjectPropertyName,
        kAudioObjectPropertyScopeGlobal, AudioObjectGetPropertyData, AudioObjectPropertyAddress,
    };
    use objc2_core_foundation::{CFRetained, CFString, CFStringBuiltInEncodings};

    let mut name_addr = AudioObjectPropertyAddress {
        mSelector: kAudioObjectPropertyName,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut name_size: u32 = std::mem::size_of::<*const c_void>() as u32;
    let mut cf_str_ptr: *const CFString = std::ptr::null();

    // SAFETY: cf_str_ptr is a valid pointer-sized out-param; name_addr is a valid stack address.
    let status = unsafe {
        AudioObjectGetPropertyData(
            dev_id,
            NonNull::new_unchecked(&mut name_addr),
            0,
            std::ptr::null(),
            NonNull::new_unchecked(&mut name_size),
            NonNull::new_unchecked(&mut cf_str_ptr as *mut *const CFString as *mut c_void),
        )
    };
    if status != kAudioHardwareNoError || cf_str_ptr.is_null() {
        return None;
    }

    // SAFETY: cf_str_ptr is a non-null, just-retained CFStringRef from CoreAudio.
    let cf_string: CFRetained<CFString> =
        unsafe { CFRetained::from_raw(NonNull::new_unchecked(cf_str_ptr as *mut CFString)) };

    let len = cf_string.length();
    let buf_len = (len as usize) * 4 + 1;
    let mut buf: Vec<u8> = vec![0u8; buf_len];
    // SAFETY: buf is a valid allocation of buf_len bytes; EncodingUTF8 is valid.
    let ok = unsafe {
        cf_string.c_string(
            buf.as_mut_ptr() as *mut i8,
            buf_len as isize,
            CFStringBuiltInEncodings::EncodingUTF8.0,
        )
    };
    if !ok {
        return None;
    }
    let c_str = std::ffi::CStr::from_bytes_until_nul(&buf).ok()?;
    Some(c_str.to_str().ok()?.to_string())
}

/// Return `true` if the CoreAudio input device whose name matches `target_name`
/// has a Bluetooth (classic or LE) transport type.
///
/// Used at stop time to decide whether to release the recording stream
/// immediately (Bluetooth — must not be pinned in HFP call mode) or keep it warm
/// (built-in / USB). Matching by NAME — the device we actually recorded from,
/// stored in `LAST_DEVICE_NAME` — is correct even when the *default* input
/// differs (e.g. the default is AirPods but recording was redirected to the
/// built-in mic). Returns `false` on any FFI failure so the warm path is kept.
pub fn device_name_is_bluetooth(target_name: &str) -> bool {
    use core::ffi::c_void;
    use core::ptr::NonNull;
    use objc2_core_audio::{
        kAudioHardwareNoError, kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, AudioObjectGetPropertyData,
        AudioObjectGetPropertyDataSize, AudioObjectPropertyAddress,
    };

    let sys_obj = kAudioObjectSystemObject as u32;
    let mut devices_address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut devices_size: u32 = 0;
    // SAFETY: devices_address and devices_size are valid stack allocations.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            sys_obj,
            NonNull::new_unchecked(&mut devices_address),
            0,
            std::ptr::null(),
            NonNull::new_unchecked(&mut devices_size),
        )
    };
    if status != kAudioHardwareNoError {
        return false;
    }
    let device_count = devices_size as usize / std::mem::size_of::<u32>();
    if device_count == 0 {
        return false;
    }
    let mut device_ids: Vec<u32> = vec![0u32; device_count];
    // SAFETY: device_ids matches the confirmed byte size.
    let status = unsafe {
        AudioObjectGetPropertyData(
            sys_obj,
            NonNull::new_unchecked(&mut devices_address),
            0,
            std::ptr::null(),
            NonNull::new_unchecked(&mut devices_size),
            NonNull::new_unchecked(device_ids.as_mut_ptr() as *mut c_void),
        )
    };
    if status != kAudioHardwareNoError {
        return false;
    }

    for &dev_id in &device_ids {
        let Some(transport) = device_transport_type(dev_id) else {
            continue;
        };
        if transport != transport::BLUETOOTH && transport != transport::BLUETOOTH_LE {
            continue;
        }
        if read_device_name(dev_id).as_deref() == Some(target_name) {
            tracing::debug!(
                "CoreAudio: recording device '{}' has Bluetooth transport",
                target_name
            );
            return true;
        }
    }
    false
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

    #[test]
    fn test_microphone_status_from_i64() {
        assert_eq!(MicrophoneStatus::from(0), MicrophoneStatus::NotDetermined);
        assert_eq!(MicrophoneStatus::from(1), MicrophoneStatus::Restricted);
        assert_eq!(MicrophoneStatus::from(2), MicrophoneStatus::Denied);
        assert_eq!(MicrophoneStatus::from(3), MicrophoneStatus::Authorized);
        // Out-of-range values map to Unknown
        assert_eq!(MicrophoneStatus::from(99), MicrophoneStatus::Unknown);
        assert_eq!(MicrophoneStatus::from(-1), MicrophoneStatus::Unknown);
    }

    #[test]
    fn test_default_input_transport_is_bluetooth() {
        // Verifies the function completes without panicking on real hardware.
        // Value depends on the test runner's connected devices — not asserted.
        let _result = default_input_transport_is_bluetooth();
    }

    #[test]
    fn test_builtin_input_device_name() {
        // Verifies the function completes without panicking.
        // A macOS machine will typically have a built-in mic; CI may return None.
        let _name = builtin_input_device_name();
    }
}
