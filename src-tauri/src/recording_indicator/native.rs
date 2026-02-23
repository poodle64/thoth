//! Native window management and software rendering for recording indicator
//!
//! This module provides platform-specific native window creation with direct
//! software rendering using tiny-skia, eliminating WebView overhead and IPC
//! complexity for the recording indicator.
//!
//! # Architecture
//!
//! Uses platform-specific window APIs directly (no winit/event loop):
//! - macOS: NSWindow via objc2-app-kit
//! - Windows: TODO - or keep WebView fallback
//! - Linux: TODO - or keep WebView fallback
//!
//! This matches industry practice (Discord, OBS, Zoom) and allows running
//! on a background thread without event loop conflicts.
//!
//! # Components
//!
//! - `SoftwareRenderer`: Cross-platform pixmap rendering via tiny-skia
//! - `NativeIndicator`: Platform-specific window management and blitting

use parking_lot::Mutex;
use std::sync::Arc;
use tiny_skia::{Paint, PathBuilder, Pixmap, Rect, Transform};

#[cfg(target_os = "macos")]
use core_graphics::base::CGFloat;
#[cfg(target_os = "macos")]
use core_graphics::color_space::CGColorSpace;
#[cfg(target_os = "macos")]
use core_graphics::context::CGContext;
#[cfg(target_os = "macos")]
use core_graphics::data_provider::CGDataProvider;
#[cfg(target_os = "macos")]
use core_graphics::image::CGImage;
#[cfg(target_os = "macos")]
use objc2::msg_send;
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSGraphicsContext, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

/// Indicator window dimensions for pill style (logical pixels)
const PILL_WIDTH: u32 = 280;
const PILL_HEIGHT: u32 = 44;

/// Indicator window dimensions for dot style (logical pixels)
const DOT_WIDTH: u32 = 58;
const DOT_HEIGHT: u32 = 58;

/// Indicator style (matches config::IndicatorStyle but defined here to avoid circular deps)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorStyle {
    Pill,
    CursorDot,
    FixedFloat,
}

/// Software renderer using tiny-skia for cross-platform CPU rendering
///
/// Renders the recording indicator graphics to an in-memory pixmap.
/// This pixmap can then be blitted to a native window surface using
/// platform-specific APIs (Core Graphics on macOS, GDI+/Direct2D on Windows,
/// X11/Wayland on Linux).
pub struct SoftwareRenderer {
    pixmap: Pixmap,
    audio_level_rms: f32,
    audio_level_peak: f32,
}

impl SoftwareRenderer {
    /// Create a new software renderer with the specified dimensions
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        let pixmap =
            Pixmap::new(width, height).ok_or_else(|| anyhow::anyhow!("Failed to create pixmap"))?;

        Ok(Self {
            pixmap,
            audio_level_rms: 0.0,
            audio_level_peak: 0.0,
        })
    }

    /// Update audio levels for visualisation
    pub fn update_audio_level(&mut self, rms: f32, peak: f32) {
        self.audio_level_rms = rms.clamp(0.0, 1.0);
        self.audio_level_peak = peak.clamp(0.0, 1.0);
    }

    /// Get the rendered pixmap data (RGBA8 format)
    ///
    /// This can be used to blit the indicator to a window surface.
    pub fn pixmap_data(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// Get pixmap dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.pixmap.width(), self.pixmap.height())
    }

    /// Render the indicator to the pixmap
    pub fn render(&mut self, style: IndicatorStyle) {
        // Clear to transparent
        self.pixmap.fill(tiny_skia::Color::TRANSPARENT);

        match style {
            IndicatorStyle::Pill => self.render_pill(),
            IndicatorStyle::CursorDot | IndicatorStyle::FixedFloat => self.render_dot(),
        }
    }

    /// Render the pill-style indicator (280x44px)
    fn render_pill(&mut self) {
        let width = self.pixmap.width() as f32;
        let height = self.pixmap.height() as f32;

        // Background: semi-transparent dark rounded rectangle
        let bg_paint = Paint {
            shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::from_rgba8(30, 30, 35, 230)),
            anti_alias: true,
            ..Default::default()
        };

        // Note: PathBuilder::from_rect returns a Path directly (not Option<Path>)
        let path = PathBuilder::from_rect(Rect::from_xywh(0.0, 0.0, width, height).unwrap());
        self.pixmap.fill_path(
            &path,
            &bg_paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );

        // Microphone icon placeholder (simple circle for now)
        let icon_paint = Paint {
            shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::from_rgba8(200, 200, 200, 255)),
            anti_alias: true,
            ..Default::default()
        };

        let mut pb = PathBuilder::new();
        pb.push_circle(20.0, height / 2.0, 8.0);
        if let Some(icon_path) = pb.finish() {
            self.pixmap.fill_path(
                &icon_path,
                &icon_paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }

        // Audio level visualisation placeholder: simple bar
        let level_height = self.audio_level_rms * (height - 8.0);
        let level_paint = Paint {
            shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::from_rgba8(0, 122, 255, 200)),
            anti_alias: true,
            ..Default::default()
        };

        let level_path = PathBuilder::from_rect(
            Rect::from_xywh(
                40.0,
                (height - level_height) / 2.0,
                width - 50.0,
                level_height,
            )
            .unwrap(),
        );
        self.pixmap.fill_path(
            &level_path,
            &level_paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    /// Render the dot-style indicator (58x58px)
    fn render_dot(&mut self) {
        let width = self.pixmap.width() as f32;
        let height = self.pixmap.height() as f32;
        let centre_x = width / 2.0;
        let centre_y = height / 2.0;

        // Outer glow based on audio level
        let glow_radius = 26.0 + (self.audio_level_peak * 6.0);
        let glow_alpha = (100.0 + self.audio_level_peak * 100.0) as u8;
        let glow_paint = Paint {
            shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::from_rgba8(
                0, 122, 255, glow_alpha,
            )),
            anti_alias: true,
            ..Default::default()
        };

        let mut pb = PathBuilder::new();
        pb.push_circle(centre_x, centre_y, glow_radius);
        if let Some(glow_path) = pb.finish() {
            self.pixmap.fill_path(
                &glow_path,
                &glow_paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }

        // Main circle
        let main_paint = Paint {
            shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::from_rgba8(30, 30, 35, 230)),
            anti_alias: true,
            ..Default::default()
        };

        let mut pb = PathBuilder::new();
        pb.push_circle(centre_x, centre_y, 22.0);
        if let Some(main_path) = pb.finish() {
            self.pixmap.fill_path(
                &main_path,
                &main_paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }

        // Microphone icon (simple circle placeholder)
        let icon_paint = Paint {
            shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::from_rgba8(200, 200, 200, 255)),
            anti_alias: true,
            ..Default::default()
        };

        let mut pb = PathBuilder::new();
        pb.push_circle(centre_x, centre_y, 8.0);
        if let Some(icon_path) = pb.finish() {
            self.pixmap.fill_path(
                &icon_path,
                &icon_paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

// =============================================================================
// macOS Native Window Implementation
// =============================================================================

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    /// Native indicator window for macOS using NSWindow
    ///
    /// Creates a borderless, always-on-top window and blits the SoftwareRenderer
    /// pixmap to it using Core Graphics.
    pub struct NativeIndicator {
        window: Retained<NSWindow>,
        renderer: SoftwareRenderer,
        style: IndicatorStyle,
    }

    impl NativeIndicator {
        /// Create a new native indicator window
        ///
        /// The window is created off-screen and invisible by default.
        /// Call `show()` to position and display it.
        pub fn new(style: IndicatorStyle) -> anyhow::Result<Self> {
            let (width, height) = match style {
                IndicatorStyle::Pill => (PILL_WIDTH, PILL_HEIGHT),
                IndicatorStyle::CursorDot | IndicatorStyle::FixedFloat => (DOT_WIDTH, DOT_HEIGHT),
            };

            let renderer = SoftwareRenderer::new(width, height)?;
            let window = Self::create_window(width as f64, height as f64)?;

            Ok(Self {
                window,
                renderer,
                style,
            })
        }

        /// Create an NSWindow with the appropriate properties for an overlay
        fn create_window(width: f64, height: f64) -> anyhow::Result<Retained<NSWindow>> {
            let rect = NSRect::new(
                NSPoint::new(-10000.0, -10000.0), // Start off-screen
                NSSize::new(width, height),
            );

            // Get main thread marker (window creation must be on main thread)
            let mtm = unsafe { MainThreadMarker::new_unchecked() };

            unsafe {
                let window = NSWindow::initWithContentRect_styleMask_backing_defer(
                    mtm.alloc(),
                    rect,
                    NSWindowStyleMask::Borderless,
                    NSBackingStoreType::Buffered,
                    false,
                );

                // Configure window as overlay
                window.setLevel(19); // NSScreenSaverWindowLevel (always on top)
                window.setOpaque(false);
                window.setBackgroundColor(Some(&NSColor::clearColor()));
                window.setHasShadow(false);
                window.setIgnoresMouseEvents(true);
                window.setCollectionBehavior(
                    NSWindowCollectionBehavior::CanJoinAllSpaces
                        | NSWindowCollectionBehavior::FullScreenAuxiliary
                        | NSWindowCollectionBehavior::IgnoresCycle,
                );

                Ok(window)
            }
        }

        /// Show the indicator at the specified position (logical pixels)
        pub fn show(&mut self, x: f64, y: f64) {
            let (width, height) = self.renderer.dimensions();
            let rect = NSRect::new(NSPoint::new(x, y), NSSize::new(width as f64, height as f64));

            unsafe {
                self.window.setFrame_display(rect, true);
                self.window.orderFront(None);

                // Render and blit
                self.render_and_blit();
            }

            tracing::debug!("Native indicator shown at ({}, {})", x, y);
        }

        /// Hide the indicator by moving it off-screen
        pub fn hide(&mut self) {
            let (width, height) = self.renderer.dimensions();
            let rect = NSRect::new(
                NSPoint::new(-10000.0, -10000.0),
                NSSize::new(width as f64, height as f64),
            );

            unsafe {
                self.window.setFrame_display(rect, false);
            }

            tracing::debug!("Native indicator hidden");
        }

        /// Update audio levels and re-render
        pub fn update_audio(&mut self, rms: f32, peak: f32) {
            self.renderer.update_audio_level(rms, peak);
            self.render_and_blit();
        }

        /// Change the indicator style
        pub fn set_style(&mut self, style: IndicatorStyle) -> anyhow::Result<()> {
            if self.style == style {
                return Ok(());
            }

            self.style = style;

            let (width, height) = match style {
                IndicatorStyle::Pill => (PILL_WIDTH, PILL_HEIGHT),
                IndicatorStyle::CursorDot | IndicatorStyle::FixedFloat => (DOT_WIDTH, DOT_HEIGHT),
            };

            // Recreate renderer with new dimensions
            self.renderer = SoftwareRenderer::new(width, height)?;

            // Resize window
            unsafe {
                let frame = self.window.frame();
                let new_frame = NSRect::new(frame.origin, NSSize::new(width as f64, height as f64));
                self.window.setFrame_display(new_frame, true);
            }

            self.render_and_blit();
            tracing::debug!("Native indicator style changed to {:?}", style);

            Ok(())
        }

        /// Render the current frame and blit to window
        fn render_and_blit(&mut self) {
            self.renderer.render(self.style);
            unsafe {
                self.blit_to_window();
            }
        }

        /// Blit the pixmap to the NSWindow using Core Graphics
        ///
        /// This creates a CGImage from the pixmap data and draws it to the window's
        /// graphics context.
        unsafe fn blit_to_window(&self) {
            let pixmap_data = self.renderer.pixmap_data();
            let (width, height) = self.renderer.dimensions();

            // Create CGImage from pixmap data
            let color_space = CGColorSpace::create_device_rgb();
            let data_provider = CGDataProvider::from_buffer(Arc::new(pixmap_data.to_vec()));

            // CGImage constants for RGBA format
            const CG_IMAGE_ALPHA_LAST: u32 = 1;
            const CG_BITMAP_BYTE_ORDER_DEFAULT: u32 = 0;

            let cg_image = CGImage::new(
                width as usize,
                height as usize,
                8,                  // bits per component
                32,                 // bits per pixel (RGBA)
                width as usize * 4, // bytes per row
                &color_space,
                CG_IMAGE_ALPHA_LAST | CG_BITMAP_BYTE_ORDER_DEFAULT,
                &data_provider,
                false, // should interpolate
                0,     // kCGRenderingIntentDefault
            );

            // Get the window's content view and lock focus
            if let Some(content_view) = self.window.contentView() {
                let _: () = msg_send![&content_view, lockFocus];

                // Get the current NSGraphicsContext
                if let Some(ns_context) = NSGraphicsContext::currentContext() {
                    // Get the underlying CGContext
                    let cg_context_ptr: *mut std::ffi::c_void = msg_send![&ns_context, CGContext];
                    let cg_context = CGContext::from_existing_context_ptr(
                        cg_context_ptr as *mut core_graphics::sys::CGContext,
                    );

                    // Clear and draw
                    cg_context.clear_rect(core_graphics::geometry::CGRect::new(
                        &core_graphics::geometry::CGPoint::new(0.0, 0.0),
                        &core_graphics::geometry::CGSize::new(width as CGFloat, height as CGFloat),
                    ));

                    cg_context.draw_image(
                        core_graphics::geometry::CGRect::new(
                            &core_graphics::geometry::CGPoint::new(0.0, 0.0),
                            &core_graphics::geometry::CGSize::new(
                                width as CGFloat,
                                height as CGFloat,
                            ),
                        ),
                        &cg_image,
                    );
                }

                let _: () = msg_send![&content_view, unlockFocus];
            }

            self.window.flushWindow();
        }
    }

    impl Drop for NativeIndicator {
        fn drop(&mut self) {
            unsafe {
                self.window.close();
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::NativeIndicator;

// =============================================================================
// Global Instance
// =============================================================================

// TODO: NSWindow is not Send+Sync (must be used on main thread only).
// The global instance pattern doesn't work with objc2's thread safety.
// Solutions:
// 1. Use thread_local! instead of static (window only accessible from main thread)
// 2. Store raw pointer and mark as unsafe (caller must ensure main thread)
// 3. Integrate with Tauri's main thread event loop instead
//
// For now, commented out to allow compilation. Integration work needed in Phase 3.

// /// Global native indicator instance
// static NATIVE_INDICATOR: Mutex<Option<Arc<Mutex<NativeIndicator>>>> = Mutex::new(None);

// TODO: Public API commented out until global instance threading is resolved
// See comment above - NSWindow must be on main thread only

// /// Initialize the native indicator system
// #[cfg(target_os = "macos")]
// pub fn init_native_indicator(style: IndicatorStyle) -> anyhow::Result<()> {
//     let indicator = NativeIndicator::new(style)?;
//     *NATIVE_INDICATOR.lock() = Some(Arc::new(Mutex::new(indicator)));
//     tracing::info!("Native indicator initialized with style {:?}", style);
//     Ok(())
// }

// /// Show the native indicator at the specified position
// #[cfg(target_os = "macos")]
// pub fn show_native_indicator(x: f64, y: f64) -> anyhow::Result<()> {
//     let guard = NATIVE_INDICATOR.lock();
//     let indicator = guard
//         .as_ref()
//         .ok_or_else(|| anyhow::anyhow!("Native indicator not initialized"))?;

//     indicator.lock().show(x, y);
//     Ok(())
// }

// /// Hide the native indicator
// #[cfg(target_os = "macos")]
// pub fn hide_native_indicator() -> anyhow::Result<()> {
//     let guard = NATIVE_INDICATOR.lock();
//     let indicator = guard
//         .as_ref()
//         .ok_or_else(|| anyhow::anyhow!("Native indicator not initialized"))?;

//     indicator.lock().hide();
//     Ok(())
// }

// /// Update audio levels
// #[cfg(target_os = "macos")]
// pub fn update_native_indicator_audio(rms: f32, peak: f32) -> anyhow::Result<()> {
//     let guard = NATIVE_INDICATOR.lock();
//     let indicator = guard
//         .as_ref()
//         .ok_or_else(|| anyhow::anyhow!("Native indicator not initialized"))?;

//     indicator.lock().update_audio(rms, peak);
//     Ok(())
// }

// /// Set the indicator style
// #[cfg(target_os = "macos")]
// pub fn set_native_indicator_style(style: IndicatorStyle) -> anyhow::Result<()> {
//     let guard = NATIVE_INDICATOR.lock();
//     let indicator = guard
//         .as_ref()
//         .ok_or_else(|| antml:anyhow!("Native indicator not initialized"))?;

//     indicator.lock().set_style(style)?;
//     Ok(())
// }

// /// Shutdown the native indicator
// #[cfg(target_os = "macos")]
// pub fn shutdown_native_indicator() -> anyhow::Result<()> {
//     *NATIVE_INDICATOR.lock() = None;
//     tracing::info!("Native indicator shut down");
//     Ok(())
// }

// Stub implementations for non-macOS platforms
#[cfg(not(target_os = "macos"))]
pub fn init_native_indicator(_style: IndicatorStyle) -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "Native indicator only supported on macOS currently"
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn show_native_indicator(_x: f64, _y: f64) -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "Native indicator only supported on macOS currently"
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn hide_native_indicator() -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "Native indicator only supported on macOS currently"
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn update_native_indicator_audio(_rms: f32, _peak: f32) -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "Native indicator only supported on macOS currently"
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn set_native_indicator_style(_style: IndicatorStyle) -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "Native indicator only supported on macOS currently"
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn shutdown_native_indicator() -> anyhow::Result<()> {
    Ok(()) // No-op on non-macOS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_creation() {
        let renderer = SoftwareRenderer::new(PILL_WIDTH, PILL_HEIGHT);
        assert!(renderer.is_ok());

        let renderer = renderer.unwrap();
        assert_eq!(renderer.dimensions(), (PILL_WIDTH, PILL_HEIGHT));
    }

    #[test]
    fn test_audio_level_clamping() {
        let mut renderer = SoftwareRenderer::new(DOT_WIDTH, DOT_HEIGHT).unwrap();

        // Test clamping to valid range
        renderer.update_audio_level(1.5, -0.5);
        assert_eq!(renderer.audio_level_rms, 1.0);
        assert_eq!(renderer.audio_level_peak, 0.0);

        // Test normal values
        renderer.update_audio_level(0.5, 0.75);
        assert_eq!(renderer.audio_level_rms, 0.5);
        assert_eq!(renderer.audio_level_peak, 0.75);
    }

    #[test]
    fn test_render_pill() {
        let mut renderer = SoftwareRenderer::new(PILL_WIDTH, PILL_HEIGHT).unwrap();
        renderer.update_audio_level(0.5, 0.8);
        renderer.render(IndicatorStyle::Pill);

        // Verify pixmap has been updated (not all transparent)
        let data = renderer.pixmap_data();
        assert!(data.iter().any(|&byte| byte != 0));
    }

    #[test]
    fn test_render_dot() {
        let mut renderer = SoftwareRenderer::new(DOT_WIDTH, DOT_HEIGHT).unwrap();
        renderer.update_audio_level(0.3, 0.6);
        renderer.render(IndicatorStyle::CursorDot);

        // Verify pixmap has been updated
        let data = renderer.pixmap_data();
        assert!(data.iter().any(|&byte| byte != 0));
    }
}
