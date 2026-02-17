<script lang="ts">
  /**
   * Compact audio player for WAV playback with waveform visualisation.
   * Reads audio files from disk via Tauri fs plugin, decodes via Web Audio API.
   */
  import { readFile } from '@tauri-apps/plugin-fs';

  interface Props {
    /** Absolute filesystem path to the WAV file */
    audioPath: string;
  }

  let { audioPath }: Props = $props();

  // Playback state
  let isPlaying = $state(false);
  let currentTime = $state(0);
  let duration = $state(0);
  let isLoaded = $state(false);
  let error = $state<string | null>(null);
  let waveformData = $state<number[]>([]);

  // Audio context and nodes
  let audioContext: AudioContext | null = null;
  let audioBuffer: AudioBuffer | null = null;
  let sourceNode: AudioBufferSourceNode | null = null;
  let startOffset = 0;
  let startTime = 0;
  let animationFrame: number | null = null;
  let progressBar = $state<HTMLDivElement | null>(null);

  const WAVEFORM_BARS = 80;

  // Load audio when path changes
  $effect(() => {
    if (audioPath) {
      loadAudio();
    }

    return () => {
      cleanup();
    };
  });

  async function loadAudio() {
    error = null;
    isLoaded = false;
    cleanup();

    try {
      const bytes = await readFile(audioPath);
      audioContext = new AudioContext();
      audioBuffer = await audioContext.decodeAudioData(bytes.buffer as ArrayBuffer);
      duration = audioBuffer.duration;
      waveformData = generateWaveform(audioBuffer, WAVEFORM_BARS);
      isLoaded = true;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.includes('path not allowed') || msg.includes('not found')) {
        error = 'Audio file not available';
      } else {
        error = 'Failed to load audio';
      }
      console.error('AudioPlayer load error:', e);
    }
  }

  function generateWaveform(buffer: AudioBuffer, bars: number): number[] {
    const data = buffer.getChannelData(0);
    const blockSize = Math.floor(data.length / bars);
    const result: number[] = [];

    for (let i = 0; i < bars; i++) {
      let sum = 0;
      const start = i * blockSize;
      for (let j = start; j < start + blockSize && j < data.length; j++) {
        sum += Math.abs(data[j]);
      }
      result.push(sum / blockSize);
    }

    // Normalise to 0-1 range
    const max = Math.max(...result, 0.001);
    return result.map((v) => v / max);
  }

  function play() {
    if (!audioContext || !audioBuffer) return;

    if (audioContext.state === 'suspended') {
      audioContext.resume();
    }

    sourceNode = audioContext.createBufferSource();
    sourceNode.buffer = audioBuffer;
    sourceNode.connect(audioContext.destination);
    sourceNode.onended = handlePlaybackEnded;

    startTime = audioContext.currentTime;
    sourceNode.start(0, startOffset);
    isPlaying = true;
    updateProgress();
  }

  function pause() {
    if (!audioContext || !sourceNode) return;

    startOffset += audioContext.currentTime - startTime;
    sourceNode.onended = null;
    sourceNode.stop();
    sourceNode = null;
    isPlaying = false;
    cancelProgressUpdate();
  }

  function togglePlayPause() {
    if (isPlaying) {
      pause();
    } else {
      play();
    }
  }

  function handlePlaybackEnded() {
    isPlaying = false;
    startOffset = 0;
    currentTime = 0;
    cancelProgressUpdate();
  }

  function updateProgress() {
    if (!audioContext || !isPlaying) return;

    currentTime = startOffset + (audioContext.currentTime - startTime);
    if (currentTime >= duration) {
      currentTime = duration;
      handlePlaybackEnded();
      return;
    }

    animationFrame = requestAnimationFrame(updateProgress);
  }

  function cancelProgressUpdate() {
    if (animationFrame !== null) {
      cancelAnimationFrame(animationFrame);
      animationFrame = null;
    }
  }

  function handleProgressClick(event: MouseEvent) {
    if (!progressBar || !duration) return;

    const rect = progressBar.getBoundingClientRect();
    const fraction = Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width));
    const wasPlaying = isPlaying;

    if (isPlaying) {
      pause();
    }

    startOffset = fraction * duration;
    currentTime = startOffset;

    if (wasPlaying) {
      play();
    }
  }

  function formatTime(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function cleanup() {
    cancelProgressUpdate();
    if (sourceNode) {
      sourceNode.onended = null;
      try {
        sourceNode.stop();
      } catch {
        // Already stopped
      }
      sourceNode = null;
    }
    if (audioContext) {
      audioContext.close();
      audioContext = null;
    }
    audioBuffer = null;
    isPlaying = false;
    currentTime = 0;
    startOffset = 0;
    duration = 0;
    waveformData = [];
    isLoaded = false;
  }

  const progressFraction = $derived(duration > 0 ? currentTime / duration : 0);
</script>

{#if error}
  <div class="player player-error">
    <span class="error-text">{error}</span>
  </div>
{:else if !isLoaded}
  <div class="player player-loading">
    <span class="loading-text">Loading audio...</span>
  </div>
{:else}
  <div class="player">
    <button
      class="play-btn"
      onclick={togglePlayPause}
      type="button"
      aria-label={isPlaying ? 'Pause' : 'Play'}
    >
      {#if isPlaying}
        <svg viewBox="0 0 24 24" fill="currentColor">
          <rect x="6" y="4" width="4" height="16" rx="1"></rect>
          <rect x="14" y="4" width="4" height="16" rx="1"></rect>
        </svg>
      {:else}
        <svg viewBox="0 0 24 24" fill="currentColor">
          <polygon points="6,4 20,12 6,20"></polygon>
        </svg>
      {/if}
    </button>

    <span class="time">{formatTime(currentTime)}</span>

    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="waveform"
      bind:this={progressBar}
      onclick={handleProgressClick}
      role="slider"
      aria-label="Audio progress"
      aria-valuemin={0}
      aria-valuemax={Math.round(duration)}
      aria-valuenow={Math.round(currentTime)}
      tabindex="0"
    >
      {#each waveformData as amplitude, i}
        {@const barProgress = i / waveformData.length}
        <div
          class="bar"
          class:played={barProgress <= progressFraction}
          style:height="{Math.max(8, amplitude * 100)}%"
        ></div>
      {/each}
    </div>

    <span class="time">{formatTime(duration)}</span>
  </div>
{/if}

<style>
  .player {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--color-bg-primary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
  }

  .player-error,
  .player-loading {
    justify-content: center;
    padding: var(--spacing-sm) var(--spacing-md);
  }

  .error-text {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  .loading-text {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  .play-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: var(--radius-full, 50%);
    background: var(--color-accent);
    color: white;
    cursor: pointer;
    flex-shrink: 0;
    transition: background var(--transition-fast);
  }

  .play-btn:hover {
    background: var(--color-accent-hover);
  }

  .play-btn svg {
    width: 14px;
    height: 14px;
  }

  .time {
    font-size: 11px;
    color: var(--color-text-tertiary);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    flex-shrink: 0;
    min-width: 32px;
    text-align: center;
  }

  .waveform {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 1px;
    height: 32px;
    cursor: pointer;
    min-width: 0;
  }

  .bar {
    flex: 1;
    min-width: 1px;
    background: var(--color-border);
    border-radius: 1px;
    transition: background 0.05s;
  }

  .bar.played {
    background: var(--color-accent);
  }
</style>
