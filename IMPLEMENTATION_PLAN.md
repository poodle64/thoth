# Native Indicator Renderer - Implementation Plan

## Summary

Complete replacement of WebView-based recording indicator with native-rendered overlay using direct canvas rendering. This follows industry best practices for desktop overlays (Discord, Zoom, OBS) and eliminates current architectural issues.

## GitHub Resources

- **Milestone**: [Native Rendering for Recording Indicator](https://github.com/poodle64/thoth/milestone/1)
- **Feature Branch**: `feature/native-indicator-renderer`
- **Architecture Doc**: [`docs/architecture/native-indicator-renderer.md`](docs/architecture/native-indicator-renderer.md)

## Issues Breakdown

### [#24 - Phase 1: Add native rendering dependencies and foundation](https://github.com/poodle64/thoth/issues/24)

**Priority**: Medium | **Effort**: 4-6 hours

Add core dependencies and create basic window infrastructure:

- winit 0.30 (cross-platform window creation)
- raw-window-handle 0.6 (platform abstraction)
- tiny-skia 0.11 (software rendering)
- Basic `NativeIndicator` struct and `SoftwareRenderer`
- Static pill rendering (no animation)

**Acceptance**: Compiles on all platforms, basic pill renders, window shows/hides

### [#25 - Phase 2: Implement all indicator styles with animations](https://github.com/poodle64/thoth/issues/25)

**Priority**: Medium | **Effort**: 8-12 hours | **Depends on**: #24

Feature parity with current WebView version:

- All three styles (pill, cursor-dot, fixed-float)
- Waveform animation (32-sample circular buffer)
- Pulsing/glow effects
- Position tracking (cursor, caret, fixed)
- Microphone icon rendering
- Smooth audio level transitions

**Acceptance**: All styles work, animation smooth at ~30fps, visual quality matches or exceeds current

### [#26 - Phase 3: Integrate with audio pipeline and replace WebView](https://github.com/poodle64/thoth/issues/26)

**Priority**: High | **Effort**: 6-8 hours | **Depends on**: #25

Replace IPC events with direct rendering:

- Modify audio/preview.rs to call renderer directly (no events)
- Remove `indicator_window_ready()` command
- Remove `recording-audio-level` events
- Feature flag for native renderer (enabled by default)
- Keep WebView fallback
- Performance testing

**Acceptance**: <1% CPU, <50ms show latency, zero event failures, tests pass

### [#27 - Phase 4: Cleanup and documentation](https://github.com/poodle64/thoth/issues/27)

**Priority**: Low | **Effort**: 2-4 hours | **Depends on**: #26 + 1 release stabilisation

Remove deprecated code after validation:

- Delete WebView indicator route
- Remove event handling code
- Remove feature flag
- Update all documentation
- Add performance comparison notes

**Acceptance**: No WebView code, docs updated, migration guide exists

## Total Effort Estimate

**20-30 hours** total across 4 phases

- Phase 1: 4-6 hours (foundation)
- Phase 2: 8-12 hours (feature parity)
- Phase 3: 6-8 hours (integration)
- Phase 4: 2-4 hours (cleanup)

## Key Benefits

### Performance

- **Current**: 2-3% CPU, 1000ms first-show delay, 30 events/sec over IPC
- **Target**: <1% CPU, <50ms first-show delay, zero IPC overhead

### Reliability

- **Current**: Fragile event delivery, timing dependencies, multiple failure points
- **Target**: Direct rendering, no IPC, single failure domain

### Code Quality

- **Current**: ~1400 LOC (Rust + Svelte), complex handshake patterns
- **Target**: <500 LOC (Rust only), straightforward architecture

## Technical Approach

### Current Architecture (Problems)

```
Audio Thread → IPC Events → Tauri Router → WebView Window → JavaScript → Canvas API
```

### New Architecture

```
Audio Thread → Shared State → Render Thread → Native Window (GPU)
```

### Key Technologies

**Cross-platform**:

- `winit` - Industry-standard window creation (used by Bevy, egui, etc.)
- `tiny-skia` - Pure Rust 2D rendering (Skia subset)
- `raw-window-handle` - Platform abstraction

**Optional GPU acceleration** (Phase 3+):

- macOS: `metal-rs` for GPU rendering
- Windows: `windows-rs` with Direct2D
- Linux: Software rendering (Wayland/X11 constraints)

## Migration Strategy

1. **Phase 1-2**: Build native renderer alongside existing WebView
2. **Phase 3**: Make native default with WebView fallback (feature flag)
3. **Release 2026.3.x**: Ship with both implementations
4. **Stabilisation**: Monitor for issues, collect metrics
5. **Phase 4**: Remove WebView after confidence established (2026.4.x)

## Risk Mitigation

| Risk                             | Mitigation                                       |
| -------------------------------- | ------------------------------------------------ |
| Platform-specific rendering bugs | Start with software rendering (works everywhere) |
| GPU driver issues                | Automatic fallback to software rendering         |
| Window positioning on Wayland    | Same limitations as current (already warned)     |
| Breaking existing users          | Feature flag + fallback mechanism                |

## Success Metrics

- [ ] Indicator shows within 50ms of recording start
- [ ] <1% CPU usage during recording
- [ ] Zero event delivery failures
- [ ] Works on macOS 14+, Windows 10+, Ubuntu 22.04+
- [ ] Code size reduced by >60%

## Development Workflow

1. **All work happens on `feature/native-indicator-renderer` branch**
2. **Each phase is a separate commit** with clear message
3. **Testing after each phase** (manual + automated)
4. **PR created after Phase 3** (ready for production)
5. **Phase 4 happens after stabilisation** (separate PR)

## Current Status

- ✅ Milestone created
- ✅ Issues created and labelled
- ✅ Architecture documented
- ✅ Feature branch created
- ⏸️ **Awaiting approval to begin Phase 1**

---

**Next Steps**: Review this plan and approve to proceed with Phase 1 implementation.
