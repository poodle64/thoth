<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { listen, emit, type UnlistenFn } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';


  // Helper to emit logs to main window (since this window's console is separate)
  function indicatorLog(...args: unknown[]) {
    const message = args.map(a => typeof a === 'object' ? JSON.stringify(a) : String(a)).join(' ');
    console.log('[Indicator]', message);
    // Also emit as event so main window can see it
    emit('indicator-log', { message: `[Indicator] ${message}` }).catch(() => {});
  }

  // Immediate logging to verify JS is loading
  indicatorLog('===== PAGE SCRIPT EXECUTING =====');

  // State
  let visualizerState = $state<'idle' | 'recording' | 'processing'>('idle');
  let audioLevel = $state(0);

  // Animation state
  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;
  let animationFrame: number | null = null;

  // Smoothed glow intensity
  let glowIntensity = 0;

  // Processing animation
  let processingPhase = 0;

  // Dimensions — 58x58 window, 34px visible rounded square centred inside
  const WIDTH = 58;
  const HEIGHT = 58;
  const ICON_SIZE = 34;
  const ICON_RADIUS = 9; // rounded square corner radius
  const ICON_X = (WIDTH - ICON_SIZE) / 2;
  const ICON_Y = (HEIGHT - ICON_SIZE) / 2;
  // Accent colour (Scribe's Amber)
  const ACCENT = { r: 208, g: 139, b: 62 }; // #D08B3E

  // Event listeners
  let unlisteners: UnlistenFn[] = [];

  onMount(async () => {
    indicatorLog('===== onMount CALLED =====');
    ctx = canvas.getContext('2d');
    setupCanvas();
    animate();

    // Listen for the test event from backend
    const shownUnlisten = await listen('indicator-shown', (event) => {
      indicatorLog('===== RECEIVED indicator-shown TEST EVENT =====', event.payload);
    });
    unlisteners.push(shownUnlisten);

    // Listen for pipeline progress
    const progressUnlisten = await listen<{ state: string; message: string; deviceName?: string }>(
      'pipeline-progress',
      (event) => {
        const state = event.payload.state;
        indicatorLog('Pipeline state:', state);
        if (state === 'recording') {
          visualizerState = 'recording';
        } else if (
          state === 'transcribing' ||
          state === 'filtering' ||
          state === 'enhancing' ||
          state === 'outputting'
        ) {
          visualizerState = 'processing';
        } else {
          visualizerState = 'idle';
        }
        indicatorLog('Visualizer state now:', visualizerState);
      }
    );
    unlisteners.push(progressUnlisten);

    // Listen for audio levels
    indicatorLog('Setting up audio level listener...');
    try {
      const levelUnlisten = await listen<{ rms: number; peak: number }>(
        'recording-audio-level',
        (event) => {
          indicatorLog('RECEIVED audio level:', event.payload.rms.toFixed(4), event.payload.peak.toFixed(4));
          // Normalise and boost for visibility
          audioLevel = Math.min(1, event.payload.rms * 3);
        }
      );
      unlisteners.push(levelUnlisten);
      indicatorLog('===== Audio level listener REGISTERED SUCCESSFULLY =====');

      // Emit ready event to signal backend we're ready to receive audio levels
      await emit('indicator-ready', {});
      indicatorLog('===== EMITTED indicator-ready EVENT =====');
    } catch (err) {
      indicatorLog('FAILED to register audio level listener:', err);
    }

    // Listen for completion
    const completeUnlisten = await listen('pipeline-complete', () => {
      visualizerState = 'idle';
    });
    unlisteners.push(completeUnlisten);
  });

  onDestroy(() => {
    if (animationFrame) {
      cancelAnimationFrame(animationFrame);
    }
    for (const unlisten of unlisteners) {
      unlisten();
    }
  });

  function setupCanvas() {
    const dpr = window.devicePixelRatio || 1;
    canvas.width = WIDTH * dpr;
    canvas.height = HEIGHT * dpr;
    canvas.style.width = `${WIDTH}px`;
    canvas.style.height = `${HEIGHT}px`;
    ctx?.scale(dpr, dpr);
  }

  let animateCount = 0;
  function animate() {
    animateCount++;
    if (animateCount % 60 === 0) {
      indicatorLog('animate() heartbeat, state:', visualizerState, 'audioLevel:', audioLevel.toFixed(3));
    }

    if (!ctx) {
      animationFrame = requestAnimationFrame(animate);
      return;
    }

    // Clear canvas
    ctx.clearRect(0, 0, WIDTH, HEIGHT);

    if (visualizerState === 'recording') {
      // Smooth glow towards audio level
      const targetGlow = Math.min(1, audioLevel * 2);
      glowIntensity += (targetGlow - glowIntensity) * 0.2;
      drawGlow();
      drawRoundedSquare();
      drawMicIcon();
    } else if (visualizerState === 'processing') {
      processingPhase += 0.04;
      const pulse = Math.sin(processingPhase * Math.PI) * 0.5 + 0.5;
      drawRoundedSquare(0.6 + pulse * 0.4);
      drawMicIcon(0.6 + pulse * 0.4);
    } else {
      // Idle: draw static (window is off-screen anyway)
      drawRoundedSquare();
      drawMicIcon();
    }

    animationFrame = requestAnimationFrame(animate);
  }

  function drawGlow() {
    if (!ctx || glowIntensity < 0.05) return;

    const cx = WIDTH / 2;
    const cy = HEIGHT / 2;
    const spread = 4 + glowIntensity * 10;
    const alpha = 0.15 + glowIntensity * 0.35;

    // Outer glow
    ctx.save();
    ctx.shadowColor = `rgba(${ACCENT.r}, ${ACCENT.g}, ${ACCENT.b}, ${alpha})`;
    ctx.shadowBlur = spread;
    ctx.shadowOffsetX = 0;
    ctx.shadowOffsetY = 0;

    ctx.beginPath();
    ctx.roundRect(ICON_X, ICON_Y, ICON_SIZE, ICON_SIZE, ICON_RADIUS);
    ctx.fillStyle = `rgba(${ACCENT.r}, ${ACCENT.g}, ${ACCENT.b}, 0.01)`;
    ctx.fill();
    ctx.restore();
  }

  function drawRoundedSquare(opacity: number = 1) {
    if (!ctx) return;

    ctx.beginPath();
    ctx.roundRect(ICON_X, ICON_Y, ICON_SIZE, ICON_SIZE, ICON_RADIUS);

    ctx.fillStyle = `rgba(${ACCENT.r}, ${ACCENT.g}, ${ACCENT.b}, ${opacity})`;
    ctx.fill();
  }

  function drawMicIcon(opacity: number = 1) {
    if (!ctx) return;

    const cx = WIDTH / 2;
    const cy = HEIGHT / 2;

    ctx.save();
    ctx.translate(cx, cy);

    // Scale the 24x24 viewBox icon to fit ~20px within the 34px square
    const scale = 20 / 24;
    ctx.scale(scale, scale);
    // Shift up slightly so mic body is visually centred (the base adds weight at bottom)
    ctx.translate(0, -1);

    const white = `rgba(255, 255, 255, ${opacity})`;

    // Mic body: rounded rectangle from y=-8 to y=4, width 6 (centred)
    ctx.beginPath();
    ctx.roundRect(-3, -8, 6, 12, 3);
    ctx.fillStyle = white;
    ctx.fill();

    // Pickup arc: from (-7, 0) curving down to (7, 0) with bottom at y=4
    ctx.beginPath();
    ctx.arc(0, 0, 7, 0, Math.PI, false);
    ctx.strokeStyle = white;
    ctx.lineWidth = 2;
    ctx.lineCap = 'round';
    ctx.stroke();

    // Stand line: from (0, 7) to (0, 10)
    ctx.beginPath();
    ctx.moveTo(0, 7);
    ctx.lineTo(0, 10);
    ctx.stroke();

    // Base: from (-4, 10) to (4, 10)
    ctx.beginPath();
    ctx.moveTo(-4, 10);
    ctx.lineTo(4, 10);
    ctx.stroke();

    ctx.restore();
  }

  /**
   * Handle click — cancel recording/processing.
   */
  async function handleStop() {
    indicatorLog('Stop button clicked');
    try {
      await invoke('pipeline_cancel');
      indicatorLog('Pipeline cancel invoked');
    } catch (err) {
      indicatorLog('Failed to cancel pipeline:', err);
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="indicator-container" onclick={handleStop}>
  <canvas bind:this={canvas} class="visualizer-canvas"></canvas>
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    padding: 0;
    background: transparent !important;
    overflow: hidden;
  }

  .indicator-container {
    width: 58px;
    height: 58px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    cursor: pointer;
  }

  .visualizer-canvas {
    display: block;
    background: transparent;
  }
</style>
