<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    visualizerState: 'idle' | 'recording' | 'processing';
    level?: number;
    size?: number;
  }

  let { visualizerState = 'idle', level = 0, size = 100 }: Props = $props();

  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;
  let animationFrame: number | null = null;
  let rotation = 0;
  let rings: Array<{ radius: number; alpha: number }> = [];

  const RING_SPAWN_THRESHOLD = 0.1;
  const RING_EXPANSION_RATE = 0.8;
  const RING_FADE_RATE = 0.015;
  const BASE_RADIUS = 20;
  const MAX_RADIUS_FACTOR = 0.45;

  onMount(() => {
    ctx = canvas.getContext('2d');
    setupCanvas();
    animate();

    return () => {
      if (animationFrame) {
        cancelAnimationFrame(animationFrame);
      }
    };
  });

  function setupCanvas() {
    const dpr = window.devicePixelRatio || 1;
    canvas.width = size * dpr;
    canvas.height = size * dpr;
    canvas.style.width = `${size}px`;
    canvas.style.height = `${size}px`;
    ctx?.scale(dpr, dpr);
  }

  function animate() {
    if (!ctx) return;

    // Clear canvas
    ctx.clearRect(0, 0, size, size);

    const centerX = size / 2;
    const centerY = size / 2;

    if (visualizerState === 'idle') {
      drawIdleState(centerX, centerY);
    } else if (visualizerState === 'recording') {
      drawRecordingState(centerX, centerY);
    } else if (visualizerState === 'processing') {
      drawProcessingState(centerX, centerY);
    }

    animationFrame = requestAnimationFrame(animate);
  }

  function drawIdleState(cx: number, cy: number) {
    if (!ctx) return;

    // Draw a subtle static circle
    ctx.beginPath();
    ctx.arc(cx, cy, BASE_RADIUS, 0, Math.PI * 2);
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.3)';
    ctx.lineWidth = 2;
    ctx.stroke();
  }

  function drawRecordingState(cx: number, cy: number) {
    if (!ctx) return;

    // Spawn new ring when level crosses threshold
    if (
      level > RING_SPAWN_THRESHOLD &&
      (rings.length === 0 || rings[rings.length - 1].radius > BASE_RADIUS + 10)
    ) {
      rings = [...rings, { radius: BASE_RADIUS, alpha: Math.min(level * 2, 1) }];
    }

    // Update and draw rings
    const maxRadius = size * MAX_RADIUS_FACTOR;
    const updatedRings: Array<{ radius: number; alpha: number }> = [];

    for (const ring of rings) {
      const newRadius = ring.radius + RING_EXPANSION_RATE;
      const newAlpha = ring.alpha - RING_FADE_RATE;

      if (newAlpha > 0 && newRadius < maxRadius) {
        updatedRings.push({ radius: newRadius, alpha: newAlpha });

        ctx.beginPath();
        ctx.arc(cx, cy, newRadius, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(208, 139, 62, ${newAlpha})`;
        ctx.lineWidth = 2 + newAlpha * 2;
        ctx.stroke();
      }
    }

    rings = updatedRings;

    // Draw center circle (level indicator)
    const centerRadius = BASE_RADIUS + level * 10;
    ctx.beginPath();
    ctx.arc(cx, cy, centerRadius, 0, Math.PI * 2);
    ctx.fillStyle = `rgba(208, 139, 62, ${0.5 + level * 0.5})`;
    ctx.fill();

    // Inner glow
    ctx.beginPath();
    ctx.arc(cx, cy, centerRadius * 0.6, 0, Math.PI * 2);
    ctx.fillStyle = `rgba(222, 158, 86, ${0.3 + level * 0.4})`;
    ctx.fill();
  }

  function drawProcessingState(cx: number, cy: number) {
    if (!ctx) return;

    // Spinning arc animation
    rotation += 0.05;

    const arcLength = Math.PI * 1.2;
    const radius = BASE_RADIUS + 5;

    ctx.beginPath();
    ctx.arc(cx, cy, radius, rotation, rotation + arcLength);
    ctx.strokeStyle = 'rgba(208, 139, 62, 0.8)';
    ctx.lineWidth = 3;
    ctx.lineCap = 'round';
    ctx.stroke();

    // Second arc (opposite side)
    ctx.beginPath();
    ctx.arc(cx, cy, radius, rotation + Math.PI, rotation + Math.PI + arcLength);
    ctx.strokeStyle = 'rgba(208, 139, 62, 0.4)';
    ctx.stroke();

    // Center dot
    ctx.beginPath();
    ctx.arc(cx, cy, 5, 0, Math.PI * 2);
    ctx.fillStyle = 'rgba(208, 139, 62, 0.6)';
    ctx.fill();
  }

  // React to size changes
  $effect(() => {
    if (canvas && ctx) {
      setupCanvas();
    }
  });
</script>

<canvas bind:this={canvas} class="audio-visualizer" style:width="{size}px" style:height="{size}px"
></canvas>

<style>
  .audio-visualizer {
    display: block;
    background: transparent;
  }
</style>
