//! Native window management and software rendering for recording indicator
//!
//! This module provides cross-platform native window creation with direct
//! software rendering using tiny-skia, eliminating WebView overhead and IPC
//! complexity for the recording indicator.
//!
//! # Phase 1 Status (Current)
//!
//! This is the foundational infrastructure for native rendering. It includes:
//! - Dependencies (winit, raw-window-handle, tiny-skia)
//! - Basic rendering logic with SoftwareRenderer
//! - Placeholder window management structures
//!
//! # Limitations
//!
//! - Window creation and event loop integration not yet implemented
//! - winit 0.30 requires EventLoop to run on main thread, conflicting with Tauri
//! - Platform-specific surface blitting not implemented
//!
//! # Next Steps (Phase 2)
//!
//! - Decide on threading model (main thread integration vs separate process)
//! - Implement actual window creation and positioning
//! - Add surface blitting for macOS/Windows/Linux
//! - Wire up audio level updates from the pipeline

use tiny_skia::{Paint, PathBuilder, Pixmap, Rect, Transform};

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
