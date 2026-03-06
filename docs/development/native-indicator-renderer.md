# Native Rendering for Recording Indicator

## Overview

Replace the current WebView-based recording indicator with a native-rendered overlay window using direct canvas rendering.

## Current Architecture (Problems)

```
Audio Thread → IPC Events → Tauri Router → WebView Window → JavaScript → Canvas API
```

**Issues:**

- Event delivery fragility (timing, window lifecycle)
- IPC overhead for 30fps real-time data
- Complex handshake patterns needed
- Multiple failure points (event registration, window lookup, state sync)
- Not industry standard for desktop overlays

## Proposed Architecture

```
Audio Thread → Shared State → Render Thread → Native Window (GPU)
```

**Benefits:**

- Zero IPC for hot path
- Direct GPU rendering (Metal/D2D/Vulkan)
- Industry-standard approach
- Simpler, more reliable
- Better performance

## Technical Design

### Core Components

#### 1. Native Window Manager

```rust
// src-tauri/src/recording_indicator/native.rs

pub struct NativeIndicator {
    window: raw_window_handle::RawWindowHandle,
    renderer: Box<dyn Renderer>,
    position: AtomicPosition,
    visible: AtomicBool,
}
```

#### 2. Cross-Platform Rendering

```rust
pub trait Renderer: Send {
    fn update_audio_level(&mut self, rms: f32, peak: f32);
    fn set_style(&mut self, style: IndicatorStyle);
    fn render(&mut self) -> Result<()>;
}

// Platform implementations:
// - MetalRenderer (macOS) - GPU accelerated
// - SoftwareRenderer (fallback) - CPU via tiny-skia
```

#### 3. Audio Integration

```rust
// In audio/preview.rs - direct call, no events
pub fn emit_audio_level(level: AudioLevel) {
    NATIVE_INDICATOR.lock().unwrap()
        .update_audio_level(level.rms, level.peak);
}
```

### Dependencies

#### Required

```toml
winit = "0.30"              # Cross-platform window creation
raw-window-handle = "0.6"   # Platform abstraction
tiny-skia = "0.11"          # Software rendering fallback
```

#### Platform-Specific (Optional GPU Acceleration)

```toml
[target.'cfg(target_os = "macos")'.dependencies]
metal = "0.29"              # GPU rendering on macOS

[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = ["Win32_Graphics_Direct2D"] }

[target.'cfg(target_os = "linux")'.dependencies]
# Use software rendering (tiny-skia) - Wayland/X11 both supported
```

### Rendering Pipeline

#### Pill Style (280x44px)

```rust
fn render_pill(&mut self, canvas: &mut Canvas) {
    // Background: rounded rectangle, semi-transparent dark
    let bg = Paint {
        color: rgba(30, 30, 35, 0.9),
        anti_alias: true,
        ..Default::default()
    };
    canvas.draw_round_rect(rect, 22.0, 22.0, &bg);

    // Waveform: 32 vertical bars
    for (i, &level) in self.waveform.iter().enumerate() {
        let height = (level * 30.0).max(2.0);
        let bar_paint = Paint {
            color: accent_color.with_alpha((level * 255.0) as u8),
            ..Default::default()
        };
        canvas.draw_round_rect(bar_rect(i, height), 2.0, 2.0, &bar_paint);
    }

    // Microphone icon (SVG path)
    canvas.draw_path(&mic_icon_path(), &icon_paint);
}
```

#### Cursor Dot Style (58x58px)

```rust
fn render_dot(&mut self, canvas: &mut Canvas) {
    // Pulsing circle with glow
    let glow_intensity = self.compute_glow(self.audio_level);

    // Outer glow
    canvas.draw_circle(center, 29.0, &glow_paint(glow_intensity));

    // Main circle
    canvas.draw_circle(center, 26.0, &main_paint);

    // Icon
    canvas.draw_path(&mic_icon_path(), &icon_paint);
}
```

### Window Management

#### Position Updates

```rust
pub fn update_position(&mut self) {
    let pos = match self.style {
        IndicatorStyle::CursorDot => get_cursor_position(),
        IndicatorStyle::Pill => get_pill_position(), // top-center of screen
        IndicatorStyle::FixedFloat => get_fixed_position(config.position),
    };

    self.window.set_position(pos);
}
```

#### Platform-Specific Attributes

```rust
// macOS: NSWindow level above all apps
window.set_level(NSWindowLevel::ScreenSaverWindowLevel);

// Windows: WS_EX_TOPMOST | WS_EX_TRANSPARENT
window.set_window_long(GWL_EXSTYLE, WS_EX_TOPMOST | WS_EX_TRANSPARENT);

// Linux: _NET_WM_STATE_ABOVE hint
window.set_wm_hints(&[WmHint::Above]);
```

## Implementation Phases

### Phase 1: Foundation (Core Infrastructure)

- Add winit + tiny-skia dependencies
- Create `NativeIndicator` window manager
- Implement software renderer (works on all platforms)
- Basic pill rendering (no animation)

### Phase 2: Feature Parity

- All three indicator styles (pill, dot, fixed)
- Waveform animation with circular buffer
- Position following (cursor, caret, fixed)
- Integration with existing show/hide commands

### Phase 3: Platform Optimization (Optional)

- Metal renderer for macOS (GPU accelerated)
- Direct2D renderer for Windows
- Performance benchmarking

### Phase 4: Migration & Cleanup

- Remove WebView indicator route
- Remove IPC event handling for audio levels
- Update tests
- Documentation

## Migration Path

### Backwards Compatibility

1. Keep WebView indicator available via feature flag during transition
2. Default to native renderer, fallback to WebView if initialisation fails
3. After stabilisation period (1 release), remove WebView code

### Configuration

```rust
// No breaking changes - same config structure
pub struct IndicatorConfig {
    pub style: IndicatorStyle,  // pill | cursor-dot | fixed-float
    pub show: bool,
    pub position: RecorderPosition, // for fixed-float
}
```

### Testing Strategy

1. Unit tests for rendering logic (pixel comparison)
2. Integration tests for window lifecycle
3. Manual testing on macOS, Windows, Linux
4. Performance profiling (CPU, GPU, memory)

## Risks & Mitigations

| Risk                          | Impact | Mitigation                                       |
| ----------------------------- | ------ | ------------------------------------------------ |
| Platform-specific bugs        | High   | Start with software rendering (works everywhere) |
| GPU driver issues             | Medium | Fallback to software rendering on error          |
| Window positioning on Wayland | Low    | Already warned in UI, same limitations           |
| Breaking existing users       | Low    | Feature flag, fallback mechanism                 |

## Success Metrics

- [ ] Indicator shows within 50ms of recording start (vs current ~1000ms)
- [ ] <1% CPU usage during recording (vs current 2-3%)
- [ ] Zero event delivery failures
- [ ] Works reliably on macOS 14+, Windows 10+, Ubuntu 22.04+
- [ ] Code size: <500 LOC (vs current ~1400 LOC)

## References

- [winit documentation](https://docs.rs/winit/)
- [tiny-skia documentation](https://docs.rs/tiny-skia/)
- [metal-rs documentation](https://docs.rs/metal/)
- Industry examples: Discord overlay, OBS studio, Zoom screen share indicator
