//! macOS traffic light (window control) positioning.
//!
//! Positions the close/minimize/zoom buttons to be vertically centred
//! in a custom title bar height.

use tauri::{Runtime, WebviewWindow};

/// Position traffic lights to be vertically centred within the given header height.
///
/// The x position is the left margin, y position is calculated to centre
/// the buttons (which are ~14px tall) within the header height.
pub fn position_traffic_lights<R: Runtime>(window: &WebviewWindow<R>, x: f64, header_height: f64) {
    #[cfg(target_os = "macos")]
    {
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2::{msg_send, Encode, Encoding, RefEncode};
        use objc2_foundation::NSPoint;

        // Traffic light buttons are approximately 14px tall
        const BUTTON_HEIGHT: f64 = 14.0;

        // Calculate y position to vertically centre the buttons
        let y = (header_height - BUTTON_HEIGHT) / 2.0;

        // Define NSRect and NSSize locally with proper Encode implementations
        #[repr(C)]
        #[derive(Copy, Clone, Debug)]
        struct NSSize {
            width: f64,
            height: f64,
        }

        unsafe impl Encode for NSSize {
            const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
        }

        unsafe impl RefEncode for NSSize {
            const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
        }

        #[repr(C)]
        #[derive(Copy, Clone, Debug)]
        struct NSRect {
            origin: NSPoint,
            size: NSSize,
        }

        unsafe impl Encode for NSRect {
            const ENCODING: Encoding =
                Encoding::Struct("CGRect", &[NSPoint::ENCODING, NSSize::ENCODING]);
        }

        unsafe impl RefEncode for NSRect {
            const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
        }

        unsafe {
            let ns_window: *mut AnyObject = window
                .ns_window()
                .expect("window should have ns_window on macOS")
                as *mut AnyObject;

            // Get the three standard window buttons
            let close: Option<Retained<AnyObject>> =
                msg_send![ns_window, standardWindowButton: 0_isize]; // NSWindowCloseButton
            let miniaturize: Option<Retained<AnyObject>> =
                msg_send![ns_window, standardWindowButton: 1_isize]; // NSWindowMiniaturizeButton
            let zoom: Option<Retained<AnyObject>> =
                msg_send![ns_window, standardWindowButton: 2_isize]; // NSWindowZoomButton

            if let (Some(close), Some(miniaturize), Some(zoom)) = (close, miniaturize, zoom) {
                // Get the superview (title bar container) - close -> superview -> superview
                let title_bar: Option<Retained<AnyObject>> = msg_send![&*close, superview];
                let title_bar: Option<Retained<AnyObject>> =
                    title_bar.and_then(|v| msg_send![&*v, superview]);

                if let Some(title_bar) = title_bar {
                    // Get the window frame to calculate title bar position
                    let window_frame: NSRect = msg_send![ns_window, frame];

                    // Set title bar container height
                    let mut title_bar_rect: NSRect = msg_send![&*title_bar, frame];
                    title_bar_rect.size.height = header_height;
                    title_bar_rect.origin.y = window_frame.size.height - header_height;
                    let _: () = msg_send![&*title_bar, setFrame: title_bar_rect];

                    // Get spacing between buttons
                    let close_frame: NSRect = msg_send![&*close, frame];
                    let miniaturize_frame: NSRect = msg_send![&*miniaturize, frame];
                    let spacing = miniaturize_frame.origin.x - close_frame.origin.x;

                    // Position each button
                    let buttons = [&close, &miniaturize, &zoom];
                    for (i, button) in buttons.iter().enumerate() {
                        let new_origin = NSPoint::new(x + (i as f64 * spacing), y);
                        let _: () = msg_send![&***button, setFrameOrigin: new_origin];
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, x, header_height);
    }
}

/// Set up traffic light positioning for a window, including handlers to
/// reposition after resize and fullscreen transitions.
pub fn setup_traffic_lights<R: Runtime>(window: &WebviewWindow<R>, x: f64, header_height: f64) {
    // Do initial positioning
    position_traffic_lights(window, x, header_height);

    // Set up listener for window resize to reposition traffic lights
    let window_clone = window.clone();

    window.on_window_event(move |event| match event {
        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Focused(true) => {
            position_traffic_lights(&window_clone, x, header_height);
        }
        _ => {}
    });
}
