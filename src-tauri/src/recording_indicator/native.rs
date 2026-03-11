//! Native rendering for the recording indicator overlay.
//!
//! This module provides high-performance, platform-native rendering for the
//! recording indicator. It replaces the previous WebView-based approach with
//! direct window and graphics API usage, eliminating IPC overhead and improving
//! reliability.
//!
//! # Architecture
//!
//! - **SoftwareRenderer**: Cross-platform 2D rendering using tiny-skia
//! - **NativeIndicator**: Platform-specific window management and blitting
//! - **Thread-local storage**: NSWindow must be on main thread (macOS requirement)
//!
//! # Usage
//!
//! ```no_run
//! # fn main() -> anyhow::Result<()> {
//! use thoth_lib::recording_indicator::native::*;
//!
//! // Initialize on main thread
//! init_native_indicator(IndicatorStyle::Pill)?;
//!
//! // Show at position
//! show_native_indicator(100.0, 50.0)?;
//!
//! // Update audio levels (~30fps from audio thread -> main thread)
//! update_native_indicator_audio(0.5, 0.8)?;
//!
//! // Hide when done
//! hide_native_indicator()?;
//! # Ok(())
//! # }
//! ```

use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// Indicator visual style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorStyle {
    /// Horizontal pill with waveform (top-centre of screen)
    Pill,
    /// Small dot that follows cursor
    CursorDot,
    /// Floating widget at fixed position
    FixedFloat,
}

// =============================================================================
// Cross-Platform Software Renderer
// =============================================================================

/// Cross-platform 2D renderer using tiny-skia.
///
/// Renders to an RGBA pixmap that is then blitted to the platform window.
/// Handles all visual effects: waveform animation, microphone icon, glow effects.
pub struct SoftwareRenderer {
    pixmap: Pixmap,
    /// RMS audio level (0.0-1.0)
    audio_level_rms: f32,
    /// Peak audio level (0.0-1.0)
    audio_level_peak: f32,
    /// Waveform history (circular buffer for pill style)
    waveform_history: [f32; 32],
    /// Current write index in waveform buffer
    waveform_index: usize,
    /// Processing animation phase (for transcribing/filtering states)
    processing_phase: f32,
    /// Smoothed glow intensity for pulsing effect
    glow_intensity: f32,
}

impl SoftwareRenderer {
    /// Create a new renderer with the specified dimensions.
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        let pixmap =
            Pixmap::new(width, height).ok_or_else(|| anyhow::anyhow!("Failed to create pixmap"))?;

        Ok(Self {
            pixmap,
            audio_level_rms: 0.0,
            audio_level_peak: 0.0,
            waveform_history: [0.0; 32],
            waveform_index: 0,
            processing_phase: 0.0,
            glow_intensity: 0.0,
        })
    }

    /// Update audio levels and waveform history.
    pub fn update_audio_level(&mut self, rms: f32, peak: f32) {
        self.audio_level_rms = rms.clamp(0.0, 1.0);
        self.audio_level_peak = peak.clamp(0.0, 1.0);

        // Update waveform circular buffer
        self.waveform_history[self.waveform_index] = self.audio_level_rms;
        self.waveform_index = (self.waveform_index + 1) % self.waveform_history.len();

        // Smooth glow intensity for pulsing effect
        let target_glow = (self.audio_level_rms * 2.0).min(1.0);
        self.glow_intensity += (target_glow - self.glow_intensity) * 0.2;
    }

    /// Advance processing animation phase.
    pub fn update_processing_phase(&mut self) {
        self.processing_phase += 0.04;
        if self.processing_phase > std::f32::consts::TAU {
            self.processing_phase -= std::f32::consts::TAU;
        }
    }

    /// Render the current frame to the pixmap.
    pub fn render(&mut self, style: IndicatorStyle, state: VisualizerState) {
        // Clear to transparent
        self.pixmap.fill(Color::TRANSPARENT);

        match style {
            IndicatorStyle::Pill => self.render_pill(state),
            IndicatorStyle::CursorDot | IndicatorStyle::FixedFloat => self.render_dot(state),
        }
    }

    /// Get the rendered pixmap data (RGBA).
    pub fn pixmap_data(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// Get pixmap dimensions.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.pixmap.width(), self.pixmap.height())
    }

    // ─── Pill Rendering ──────────────────────────────────────────────────

    fn render_pill(&mut self, state: VisualizerState) {
        let width = self.pixmap.width() as f32;
        let height = self.pixmap.height() as f32;
        let radius = height / 2.0;

        // Accent colour (Scribe's Amber: #D08B3E)
        let accent = Color::from_rgba8(208, 139, 62, 255);

        match state {
            VisualizerState::Recording => {
                // Background pill
                self.draw_pill_background(width, height, radius, accent, 0.85);
                // Waveform bars
                self.draw_waveform_bars(width, height);
                // Microphone icon
                self.draw_pill_mic_icon(height);
            }
            VisualizerState::Processing => {
                self.update_processing_phase();
                let pulse = self.processing_phase.sin() * 0.5 + 0.5;
                let opacity = 0.6 + pulse * 0.25;
                self.draw_pill_background(width, height, radius, accent, opacity);
                self.draw_pill_processing_dots(width, height);
                self.draw_pill_mic_icon(height);
            }
            VisualizerState::Idle => {
                self.draw_pill_background(width, height, radius, accent, 1.0);
                self.draw_pill_mic_icon(height);
            }
        }
    }

    fn draw_pill_background(
        &mut self,
        width: f32,
        height: f32,
        radius: f32,
        color: Color,
        opacity: f32,
    ) {
        let mut paint = Paint::default();
        paint.set_color(
            Color::from_rgba(color.red(), color.green(), color.blue(), opacity).unwrap(),
        );
        paint.anti_alias = true;

        // Draw center rectangle
        let rect_path = PathBuilder::from_rect(
            tiny_skia::Rect::from_xywh(radius, 0.0, width - radius * 2.0, height).unwrap(),
        );
        self.pixmap.fill_path(
            &rect_path,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );

        // Draw left cap (circle)
        if let Some(left_cap) = PathBuilder::from_circle(radius, height / 2.0, radius) {
            self.pixmap.fill_path(
                &left_cap,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }

        // Draw right cap (circle)
        if let Some(right_cap) = PathBuilder::from_circle(width - radius, height / 2.0, radius) {
            self.pixmap.fill_path(
                &right_cap,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    fn draw_waveform_bars(&mut self, width: f32, height: f32) {
        let mic_area_width = 40.0;
        let bar_region_start = mic_area_width + 4.0;
        let bar_region_end = width - 16.0;
        let bar_region_width = bar_region_end - bar_region_start;
        let bar_count = self.waveform_history.len();
        let bar_width = 3.0;
        let bar_gap = (bar_region_width - bar_count as f32 * bar_width) / (bar_count as f32 - 1.0);
        let max_bar_height = height - 14.0;
        let cy = height / 2.0;

        let mut paint = Paint::default();
        paint.anti_alias = true;

        for i in 0..bar_count {
            // Read from circular buffer (oldest first)
            let buf_index = (self.waveform_index + i) % bar_count;
            let level = self.waveform_history[buf_index];

            // Minimum visible height + scaled height
            let bar_height = (4.0_f32).max(level * max_bar_height);
            let x = bar_region_start + i as f32 * (bar_width + bar_gap);
            let y = cy - bar_height / 2.0;

            // Fade bars from left (older) to right (newer)
            let age_factor = 0.4 + (i as f32 / bar_count as f32) * 0.6;
            paint.set_color(Color::from_rgba(1.0, 1.0, 1.0, age_factor).unwrap());

            // Rounded bar
            let rect = tiny_skia::Rect::from_xywh(x, y, bar_width, bar_height).unwrap();
            let path = PathBuilder::from_rect(rect);
            self.pixmap.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    fn draw_pill_mic_icon(&mut self, height: f32) {
        let mic_area_width = 40.0;
        let cx = mic_area_width / 2.0 + 4.0;
        let cy = height / 2.0;
        let scale = 16.0 / 24.0;

        self.draw_microphone_icon(cx, cy, scale, 1.0);
    }

    fn draw_pill_processing_dots(&mut self, width: f32, height: f32) {
        let cy = height / 2.0;
        let cx = width / 2.0 + 10.0;
        let dot_radius = 3.0;
        let dot_spacing = 14.0;

        let mut paint = Paint::default();
        paint.anti_alias = true;

        for i in 0..3 {
            let phase = self.processing_phase + i as f32 * 0.7;
            let bounce = phase.sin() * 0.5 + 0.5;
            let y = cy - bounce * 6.0;
            let alpha = 0.5 + bounce * 0.5;

            paint.set_color(Color::from_rgba(1.0, 1.0, 1.0, alpha).unwrap());
            if let Some(circle) =
                PathBuilder::from_circle(cx + (i as f32 - 1.0) * dot_spacing, y, dot_radius)
            {
                self.pixmap.fill_path(
                    &circle,
                    &paint,
                    tiny_skia::FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
    }

    // ─── Dot Rendering ───────────────────────────────────────────────────

    fn render_dot(&mut self, state: VisualizerState) {
        let width = self.pixmap.width() as f32;
        let height = self.pixmap.height() as f32;
        let icon_size = 34.0;
        let icon_radius = 9.0;
        let icon_x = (width - icon_size) / 2.0;
        let icon_y = (height - icon_size) / 2.0;

        // Accent colour
        let accent = Color::from_rgba8(208, 139, 62, 255);

        match state {
            VisualizerState::Recording => {
                // Glow effect
                if self.glow_intensity >= 0.05 {
                    self.draw_dot_glow(width, height, icon_x, icon_y, icon_size);
                }
                // Rounded square background
                self.draw_rounded_square(icon_x, icon_y, icon_size, icon_radius, accent, 1.0);
                // Microphone icon
                let cx = width / 2.0;
                let cy = height / 2.0;
                self.draw_microphone_icon(cx, cy, 20.0 / 24.0, 1.0);
            }
            VisualizerState::Processing => {
                self.update_processing_phase();
                let pulse = self.processing_phase.sin() * 0.5 + 0.5;
                let opacity = 0.6 + pulse * 0.4;
                self.draw_rounded_square(icon_x, icon_y, icon_size, icon_radius, accent, opacity);
                let cx = width / 2.0;
                let cy = height / 2.0;
                self.draw_microphone_icon(cx, cy, 20.0 / 24.0, opacity);
            }
            VisualizerState::Idle => {
                self.draw_rounded_square(icon_x, icon_y, icon_size, icon_radius, accent, 1.0);
                let cx = width / 2.0;
                let cy = height / 2.0;
                self.draw_microphone_icon(cx, cy, 20.0 / 24.0, 1.0);
            }
        }
    }

    fn draw_dot_glow(
        &mut self,
        _width: f32,
        _height: f32,
        icon_x: f32,
        icon_y: f32,
        icon_size: f32,
    ) {
        // Note: tiny-skia doesn't support shadows directly. This is a simplified glow
        // by drawing multiple larger rounded rects with decreasing opacity.
        let accent = Color::from_rgba8(208, 139, 62, 255);
        let spread = 4.0 + self.glow_intensity * 10.0;
        let base_alpha = 0.15 + self.glow_intensity * 0.35;

        let mut paint = Paint::default();
        paint.anti_alias = true;

        for i in 0..3 {
            let offset = spread * (3 - i) as f32 / 3.0;
            let alpha = base_alpha * (i + 1) as f32 / 3.0;
            paint.set_color(
                Color::from_rgba(
                    accent.red() as f32 / 255.0,
                    accent.green() as f32 / 255.0,
                    accent.blue() as f32 / 255.0,
                    alpha,
                )
                .unwrap(),
            );

            let glow_x = icon_x - offset;
            let glow_y = icon_y - offset;
            let glow_size = icon_size + offset * 2.0;

            let rect = tiny_skia::Rect::from_xywh(glow_x, glow_y, glow_size, glow_size).unwrap();
            let path = PathBuilder::from_rect(rect);
            self.pixmap.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    fn draw_rounded_square(
        &mut self,
        x: f32,
        y: f32,
        size: f32,
        _radius: f32,
        color: Color,
        opacity: f32,
    ) {
        let mut paint = Paint::default();
        paint.set_color(
            Color::from_rgba(
                color.red() as f32 / 255.0,
                color.green() as f32 / 255.0,
                color.blue() as f32 / 255.0,
                opacity,
            )
            .unwrap(),
        );
        paint.anti_alias = true;

        let rect = tiny_skia::Rect::from_xywh(x, y, size, size).unwrap();
        let path = PathBuilder::from_rect(rect);
        self.pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    fn draw_microphone_icon(&mut self, cx: f32, cy: f32, scale: f32, opacity: f32) {
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba(1.0, 1.0, 1.0, opacity).unwrap());
        paint.anti_alias = true;

        // Mic body (rounded rectangle)
        let body_width = 6.0 * scale;
        let body_height = 12.0 * scale;
        let body_x = cx - body_width / 2.0;
        let body_y = cy - 8.0 * scale - scale; // -1 translate
        let body_rect =
            tiny_skia::Rect::from_xywh(body_x, body_y, body_width, body_height).unwrap();
        let body_path = PathBuilder::from_rect(body_rect);
        self.pixmap.fill_path(
            &body_path,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );

        // Pickup arc
        let mut stroke = Stroke::default();
        stroke.width = 2.0 * scale;
        stroke.line_cap = tiny_skia::LineCap::Round;
        paint.set_color(Color::from_rgba(1.0, 1.0, 1.0, opacity).unwrap());

        let arc_path = PathBuilder::from_circle(cx, cy - scale, 7.0 * scale);
        if let Some(arc_path) = arc_path {
            self.pixmap
                .stroke_path(&arc_path, &paint, &stroke, Transform::identity(), None);
        }

        // Stand line
        let mut stand_path = PathBuilder::new();
        stand_path.move_to(cx, cy + 7.0 * scale - scale);
        stand_path.line_to(cx, cy + 10.0 * scale - scale);
        if let Some(stand_path) = stand_path.finish() {
            self.pixmap
                .stroke_path(&stand_path, &paint, &stroke, Transform::identity(), None);
        }

        // Base
        let mut base_path = PathBuilder::new();
        base_path.move_to(cx - 4.0 * scale, cy + 10.0 * scale - scale);
        base_path.line_to(cx + 4.0 * scale, cy + 10.0 * scale - scale);
        if let Some(base_path) = base_path.finish() {
            self.pixmap
                .stroke_path(&base_path, &paint, &stroke, Transform::identity(), None);
        }
    }
}

/// Visualizer state (determines which animation to show)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualizerState {
    Idle,
    Recording,
    Processing,
}

// =============================================================================
// macOS Native Window Implementation
// =============================================================================

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use core_graphics::base::{kCGBitmapByteOrderDefault, kCGImageAlphaPremultipliedLast};
    use core_graphics::color_space::CGColorSpace;
    use core_graphics::context::CGContext;
    use core_graphics::data_provider::CGDataProvider;
    use core_graphics::image::CGImage;
    use objc2::rc::Retained;
    use objc2::{msg_send, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSBackingStoreType, NSColor, NSWindow, NSWindowStyleMask};
    use objc2_foundation::NSPoint as CGPoint;
    use objc2_foundation::NSRect as CGRect;
    use objc2_foundation::NSSize as CGSize;
    use std::sync::Arc;

    /// Native macOS indicator using NSWindow.
    ///
    /// Uses platform-specific APIs for maximum performance and reliability.
    /// Must be created and accessed only on the main thread.
    pub struct NativeIndicator {
        window: Retained<NSWindow>,
        renderer: SoftwareRenderer,
        style: IndicatorStyle,
        state: VisualizerState,
    }

    impl NativeIndicator {
        /// Create a new native indicator.
        ///
        /// MUST be called from the main thread.
        pub fn new(style: IndicatorStyle) -> anyhow::Result<Self> {
            let (width, height) = match style {
                IndicatorStyle::Pill => (280, 44),
                IndicatorStyle::CursorDot | IndicatorStyle::FixedFloat => (58, 58),
            };

            let window = Self::create_window(width as f64, height as f64)?;
            let renderer = SoftwareRenderer::new(width, height)?;

            Ok(Self {
                window,
                renderer,
                style,
                state: VisualizerState::Idle,
            })
        }

        /// Create an NSWindow for the indicator.
        fn create_window(width: f64, height: f64) -> anyhow::Result<Retained<NSWindow>> {
            let mtm = unsafe { MainThreadMarker::new_unchecked() };

            let rect = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize { width, height },
            };

            let window = unsafe {
                let alloc = NSWindow::alloc(mtm);
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    alloc,
                    rect,
                    NSWindowStyleMask::Borderless,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };

            // Configure window properties
            window.setLevel(19); // NSScreenSaverWindowLevel - above everything
            window.setOpaque(false);
            window.setHasShadow(false);
            window.setIgnoresMouseEvents(true);
            window.setBackgroundColor(Some(&NSColor::clearColor()));

            // Enable transparency
            window.setAlphaValue(1.0);

            // Prevent macOS from restoring this window after sleep/wake or app restart
            // Mark as transient so macOS doesn't restore it after sleep/wake or app restart
            window.setCollectionBehavior(objc2_app_kit::NSWindowCollectionBehavior::Transient);

            Ok(window)
        }

        /// Show the indicator at the specified position.
        pub fn show(&mut self, x: f64, y: f64) {
            let origin = CGPoint { x, y };
            self.window.setFrameOrigin(origin);
            self.window.orderFrontRegardless();

            // Render initial frame
            self.renderer.render(self.style, self.state);
            unsafe { self.blit_to_window() };
        }

        /// Hide the indicator.
        pub fn hide(&mut self) {
            self.window.orderOut(None);
            // Reset to idle state for next show
            self.state = VisualizerState::Idle;
        }

        /// Update audio levels and re-render.
        pub fn update_audio(&mut self, rms: f32, peak: f32) {
            // Update audio levels in renderer (don't override state - it's managed separately)
            self.renderer.update_audio_level(rms, peak);
            self.renderer.render(self.style, self.state);
            unsafe { self.blit_to_window() };
        }

        /// Set the visualizer state.
        pub fn set_state(&mut self, state: VisualizerState) {
            self.state = state;
            self.renderer.render(self.style, self.state);
            unsafe { self.blit_to_window() };
        }

        /// Set the indicator style.
        pub fn set_style(&mut self, style: IndicatorStyle) {
            if self.style == style {
                return;
            }

            self.style = style;

            // Resize renderer and window
            let (width, height) = match style {
                IndicatorStyle::Pill => (280, 44),
                IndicatorStyle::CursorDot | IndicatorStyle::FixedFloat => (58, 58),
            };

            // Create new renderer with new dimensions
            self.renderer =
                SoftwareRenderer::new(width, height).expect("Failed to recreate renderer");

            // Resize window
            let size = CGSize {
                width: width as f64,
                height: height as f64,
            };
            self.window.setContentSize(size);

            // Re-render
            self.renderer.render(self.style, self.state);
            unsafe { self.blit_to_window() };
        }

        /// Blit the pixmap to the NSWindow's content view.
        ///
        /// # Safety
        /// Must be called from the main thread.
        #[allow(deprecated)] // lockFocus/unlockFocus still works; NSView subclass alternative is future work
        unsafe fn blit_to_window(&self) {
            let Some(content_view) = self.window.contentView() else {
                tracing::warn!("No content view for indicator window");
                return;
            };

            // Lock focus to prepare for drawing
            content_view.lockFocus();

            // Get current NSGraphicsContext
            let ns_context_ptr = {
                let ns_context_class = objc2::class!(NSGraphicsContext);
                let ns_context: *mut objc2::runtime::AnyObject =
                    msg_send![ns_context_class, currentContext];
                if ns_context.is_null() {
                    content_view.unlockFocus();
                    return;
                }
                ns_context
            };

            // Get CGContext from NSGraphicsContext
            let cg_context_ptr: *mut std::ffi::c_void = msg_send![ns_context_ptr, CGContext];
            let cg_context_ptr = cg_context_ptr as *mut core_graphics::sys::CGContext;

            // Create CGImage from pixmap
            let (width, height) = self.renderer.dimensions();
            let pixmap_data = self.renderer.pixmap_data();

            // Create data provider from pixmap bytes
            let data_provider = CGDataProvider::from_buffer(Arc::new(pixmap_data.to_vec()));

            let color_space = CGColorSpace::create_device_rgb();
            let cg_image = CGImage::new(
                width as usize,
                height as usize,
                8,                  // bits per component
                32,                 // bits per pixel (RGBA)
                width as usize * 4, // bytes per row
                &color_space,
                kCGImageAlphaPremultipliedLast | kCGBitmapByteOrderDefault,
                &data_provider,
                false,
                0, // rendering intent (0 = kCGRenderingIntentDefault)
            );

            // Draw the image
            let cg_context = CGContext::from_existing_context_ptr(cg_context_ptr);
            let rect = core_graphics::geometry::CGRect::new(
                &core_graphics::geometry::CGPoint::new(0.0, 0.0),
                &core_graphics::geometry::CGSize::new(width as f64, height as f64),
            );

            cg_context.draw_image(rect, &cg_image);

            // Unlock focus
            content_view.unlockFocus();

            // Force display
            content_view.setNeedsDisplay(true);
        }
    }

    impl Drop for NativeIndicator {
        fn drop(&mut self) {
            self.window.close();
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::NativeIndicator;

// =============================================================================
// Global Instance - Thread-Local Pattern
// =============================================================================

// NSWindow must be created and accessed only on the main thread. We use
// thread_local! to ensure this constraint is enforced at compile time.
// The indicator is created lazily on first use and persists for the app lifetime.

use std::cell::RefCell;

thread_local! {
    /// Thread-local native indicator instance (main thread only)
    static NATIVE_INDICATOR: RefCell<Option<NativeIndicator>> = const { RefCell::new(None) };
}

/// Initialize the native indicator system.
///
/// MUST be called from the main thread. Creates the NSWindow and renderer.
#[cfg(target_os = "macos")]
pub fn init_native_indicator(style: IndicatorStyle) -> anyhow::Result<()> {
    NATIVE_INDICATOR.with(|cell| {
        let indicator = NativeIndicator::new(style)?;
        *cell.borrow_mut() = Some(indicator);
        tracing::info!("Native indicator initialized with style {:?}", style);
        Ok(())
    })
}

/// Show the native indicator at the specified position.
///
/// MUST be called from the main thread.
#[cfg(target_os = "macos")]
pub fn show_native_indicator(x: f64, y: f64) -> anyhow::Result<()> {
    NATIVE_INDICATOR.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let indicator = borrowed
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Native indicator not initialized"))?;
        indicator.show(x, y);
        Ok(())
    })
}

/// Hide the native indicator.
///
/// MUST be called from the main thread.
#[cfg(target_os = "macos")]
pub fn hide_native_indicator() -> anyhow::Result<()> {
    NATIVE_INDICATOR.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let indicator = borrowed
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Native indicator not initialized"))?;
        indicator.hide();
        Ok(())
    })
}

/// Update audio levels.
///
/// MUST be called from the main thread.
#[cfg(target_os = "macos")]
pub fn update_native_indicator_audio(rms: f32, peak: f32) -> anyhow::Result<()> {
    NATIVE_INDICATOR.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let indicator = borrowed
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Native indicator not initialized"))?;
        indicator.update_audio(rms, peak);
        Ok(())
    })
}

/// Poll for audio level updates and apply them to the native indicator.
///
/// This should be called periodically from the main thread (via Tauri command
/// from frontend). It drains the audio level channel and updates the indicator
/// with the latest levels.
///
/// MUST be called from the main thread (Tauri commands run on main thread).
#[tauri::command]
#[cfg(all(feature = "native-indicator", target_os = "macos"))]
pub fn poll_native_indicator_audio() -> Result<(), String> {
    if let Some((rms, peak)) = crate::audio::poll_recording_audio_levels() {
        update_native_indicator_audio(rms, peak).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Set the visualizer state.
///
/// MUST be called from the main thread.
#[cfg(target_os = "macos")]
pub fn set_native_indicator_state(state: VisualizerState) -> anyhow::Result<()> {
    NATIVE_INDICATOR.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let indicator = borrowed
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Native indicator not initialized"))?;
        indicator.set_state(state);
        Ok(())
    })
}

/// Set the indicator style.
///
/// MUST be called from the main thread.
#[cfg(target_os = "macos")]
pub fn set_native_indicator_style(style: IndicatorStyle) -> anyhow::Result<()> {
    NATIVE_INDICATOR.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let indicator = borrowed
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Native indicator not initialized"))?;
        indicator.set_style(style);
        Ok(())
    })
}

/// Shutdown the native indicator.
///
/// MUST be called from the main thread.
#[cfg(target_os = "macos")]
pub fn shutdown_native_indicator() -> anyhow::Result<()> {
    NATIVE_INDICATOR.with(|cell| {
        *cell.borrow_mut() = None;
        tracing::info!("Native indicator shut down");
        Ok(())
    })
}

// =============================================================================
// Stub Implementations for Non-macOS Platforms
// =============================================================================

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
pub fn set_native_indicator_state(_state: VisualizerState) -> anyhow::Result<()> {
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

/// No-op stub for non-macOS platforms
#[tauri::command]
#[cfg(not(all(feature = "native-indicator", target_os = "macos")))]
pub fn poll_native_indicator_audio() -> Result<(), String> {
    // No-op on non-macOS platforms or when native-indicator feature is disabled
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn shutdown_native_indicator() -> anyhow::Result<()> {
    Ok(()) // No-op on non-macOS
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_creation() {
        let renderer = SoftwareRenderer::new(280, 44).unwrap();
        assert_eq!(renderer.dimensions(), (280, 44));
    }

    #[test]
    fn test_audio_level_clamping() {
        let mut renderer = SoftwareRenderer::new(100, 100).unwrap();
        renderer.update_audio_level(1.5, 2.0); // Above 1.0
        assert_eq!(renderer.audio_level_rms, 1.0);
        assert_eq!(renderer.audio_level_peak, 1.0);

        renderer.update_audio_level(-0.5, -0.1); // Below 0.0
        assert_eq!(renderer.audio_level_rms, 0.0);
        assert_eq!(renderer.audio_level_peak, 0.0);
    }

    #[test]
    fn test_waveform_circular_buffer() {
        let mut renderer = SoftwareRenderer::new(100, 100).unwrap();

        // Fill buffer
        for i in 0..32 {
            renderer.update_audio_level(i as f32 / 32.0, 0.0);
        }

        // Check it wraps around
        assert_eq!(renderer.waveform_index, 0);
        renderer.update_audio_level(0.5, 0.0);
        assert_eq!(renderer.waveform_index, 1);
    }

    #[test]
    fn test_rendering_produces_output() {
        let mut renderer = SoftwareRenderer::new(280, 44).unwrap();
        renderer.render(IndicatorStyle::Pill, VisualizerState::Recording);

        // Check that pixmap has non-zero data
        let data = renderer.pixmap_data();
        assert_eq!(data.len(), 280 * 44 * 4); // RGBA
    }
}
