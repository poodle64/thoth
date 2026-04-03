<script lang="ts">
  /**
   * Overview pane - landing page for Settings showing stats and status at a glance.
   *
   * Displays summary cards, model performance, and system status.
   * Data refreshes automatically on each pane visit (component remounts).
   */
	import { onMount, onDestroy } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { getVersion } from '@tauri-apps/api/app';
	import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart';
	import { configStore } from '../stores/config.svelte';
	import { settingsStore } from '../stores/settings.svelte';
	import {
		formatDuration,
		formatTotalDuration
	} from '../utils/format';
	import { getUpdaterState, checkForUpdate } from '../stores/updater.svelte';

  interface ModelStats {
    name: string;
    count: number;
    avgAudioDuration: number;
    avgProcessingTime: number;
    speedFactor: number;
  }

  interface TranscriptionStats {
    totalCount: number;
    analysableCount: number;
    enhancedCount: number;
    totalAudioDuration: number;
    transcriptionModels: ModelStats[];
    enhancementModels: ModelStats[];
  }

  interface DetectedGpu {
    backend: string;
    name: string;
    vram_mb: number | null;
  }

  interface GpuInfo {
    compiled_backend: string;
    gpu_available: boolean;
    gpu_name: string | null;
    vram_mb: number | null;
    detected_gpus: DetectedGpu[];
  }

  interface Props {
    /** Callback to navigate to another Settings pane */
    onNavigate: (paneId: string) => void;
  }

	let { onNavigate }: Props = $props();

	let stats = $state<TranscriptionStats | null>(null);
	let transcriptionReady = $state(false);
	let modelDownloaded = $state(false);
	let isLoading = $state(true);
	let ollamaStatus = $state<'checking' | 'connected' | 'unavailable' | 'not-configured'>(
		'checking'
	);
	let gpuInfo = $state<GpuInfo | null>(null);

	const updaterState = getUpdaterState();
	let currentVersion = $state('');

  /** Average recording duration */
  let avgRecordingDuration = $derived(
    stats && stats.analysableCount > 0
      ? stats.totalAudioDuration / stats.analysableCount
      : 0
  );

  /** Selected device display name */
  let deviceName = $derived.by(() => {
    const deviceId = settingsStore.selectedDeviceId;
    if (!deviceId) {
      const defaultDevice = settingsStore.audioDevices.find((d) => d.is_default);
      return defaultDevice?.name ?? 'System Default';
    }
    const device = settingsStore.audioDevices.find((d) => d.id === deviceId);
    return device?.name ?? 'Unknown Device';
  });

  /** Autostart (launch at login) state */
  let autostartEnabled = $state(false);
  let autostartLoading = $state(false);
  let autostartError = $state<string | null>(null);

  /** Show in Dock state (macOS) */
  let showInDock = $state(false);
  let dockLoading = $state(false);

  /** Permission states */
  let microphonePermission = $state<'unknown' | 'granted' | 'denied' | 'not_determined'>('unknown');
  let accessibilityPermission = $state<'unknown' | 'granted' | 'denied' | 'stale'>('unknown');
  let inputMonitoringPermission = $state<'unknown' | 'granted' | 'denied'>('unknown');

  /** Whether all permissions are granted (and functional) */
  let allPermissionsGranted = $derived(
    microphonePermission === 'granted' &&
    accessibilityPermission === 'granted' &&
    inputMonitoringPermission === 'granted'
  );

  /** TCC reset state */
  let resettingPermissions = $state(false);
  let resetError = $state<string | null>(null);

  let permFixStatus = $state<{ quarantine: string; tcc: string }>({ quarantine: '', tcc: '' });


  async function handleAutostartToggle() {
    autostartLoading = true;
    autostartError = null;

    try {
      if (autostartEnabled) {
        await disable();
        autostartEnabled = false;
      } else {
        await enable();
        autostartEnabled = true;
      }
    } catch (error) {
      autostartError =
        error instanceof Error ? error.message : 'Failed to update autostart setting';
      console.error('Autostart toggle failed:', error);
    } finally {
      autostartLoading = false;
    }
  }

  async function loadAutostartState() {
    try {
      autostartEnabled = await isEnabled();
    } catch (error) {
      console.error('Failed to check autostart status:', error);
      autostartError = 'Failed to check autostart status';
    }
  }

  async function loadDockState() {
    try {
      showInDock = await invoke<boolean>('get_show_in_dock');
    } catch (error) {
      console.error('Failed to check dock state:', error);
    }
  }

  async function handleDockToggle() {
    dockLoading = true;
    try {
      const newValue = !showInDock;
      await invoke('set_show_in_dock', { show: newValue });
      showInDock = newValue;
    } catch (error) {
      console.error('Failed to toggle dock visibility:', error);
    } finally {
      dockLoading = false;
    }
  }

  let permissionChangedUnlisten: UnlistenFn | null = null;

  async function checkPermissions() {
    try {
      const micStatus = await invoke<string>('check_microphone_permission');
      if (micStatus === 'granted') {
        microphonePermission = 'granted';
      } else if (micStatus === 'not_determined') {
        microphonePermission = 'not_determined';
      } else {
        microphonePermission = 'denied';
      }
    } catch (error) {
      console.error('Failed to check microphone permission:', error);
      microphonePermission = 'unknown';
    }

    try {
      const accessStatus = await invoke<boolean>('check_accessibility');
      if (accessStatus) {
        // Permission entry exists — verify it's actually functional
        const functional = await invoke<boolean>('verify_accessibility_functional');
        accessibilityPermission = functional ? 'granted' : 'stale';
      } else {
        accessibilityPermission = 'denied';
      }
    } catch (error) {
      console.error('Failed to check accessibility:', error);
      accessibilityPermission = 'unknown';
    }

    try {
      const inputStatus = await invoke<boolean>('check_input_monitoring');
      const wasNotGranted = inputMonitoringPermission !== 'granted';
      inputMonitoringPermission = inputStatus ? 'granted' : 'denied';

      // If Input Monitoring was just granted, start the keyboard service
      // so modifier shortcuts work without requiring an app restart.
      if (wasNotGranted && inputStatus) {
        invoke('try_start_keyboard_service').catch(() => {});
      }
    } catch (error) {
      console.error('Failed to check input monitoring:', error);
      inputMonitoringPermission = 'unknown';
    }
  }

  /** Reset TCC permissions for Thoth (requires admin privileges) */
  async function resetPermissions(services: string[]) {
    resettingPermissions = true;
    resetError = null;


    try {
      await invoke<string>('reset_tcc_permissions', { services });
      await checkPermissions();
      startPermissionPoll();
    } catch (error) {
      resetError = error instanceof Error ? error.message : String(error);
      console.error('Failed to reset TCC permissions:', error);
    } finally {
      resettingPermissions = false;
    }
  }

  /**
   * Adaptive permission polling — the standard macOS pattern.
   *
   * macOS provides no callback for TCC permission changes (Accessibility,
   * Input Monitoring). Even the microphone dialog triggered by CoreAudio
   * has no completion handler. Every well-built macOS app (AltTab, Rectangle,
   * Raycast) polls AXIsProcessTrusted() on a timer.
   *
   * We poll at 500ms while permissions are outstanding (fast enough to feel
   * instant when the user toggles a switch in System Settings) and stop
   * the moment all permissions are granted.
   */
  const POLL_MS = 500;
  let permissionPollTimer: ReturnType<typeof setInterval> | null = null;

  function startPermissionPoll() {
    if (permissionPollTimer !== null) return;
    permissionPollTimer = setInterval(async () => {
      await checkPermissions();
      if (allPermissionsGranted) {
        stopPermissionPoll();
        invoke('refresh_tray_menu').catch(() => {});
      }
    }, POLL_MS);
  }

  function stopPermissionPoll() {
    if (permissionPollTimer !== null) {
      clearInterval(permissionPollTimer);
      permissionPollTimer = null;
    }
  }

  /** Request a permission (opens system dialog or System Settings) */
  function requestPermission(command: string) {
    invoke(command);
    startPermissionPoll();
  }

  /** Model download state for setup card */
  type SetupState = 'needed' | 'downloading' | 'initialising' | 'ready' | 'error';
  let setupState = $state<SetupState>('ready');
  let downloadProgress = $state(0);
  let downloadError = $state<string | null>(null);
  let downloadUnlisteners: UnlistenFn[] = [];

  /** Setup step status tracking */
  let modelStepDone = $derived(setupState === 'ready');
  let micStepDone = $derived(microphonePermission === 'granted');
  let shortcutStepDone = $derived(accessibilityPermission === 'granted');
  let accessibilityStale = $derived(accessibilityPermission === 'stale');
  let allRequiredDone = $derived(modelStepDone && micStepDone && shortcutStepDone);

  /** Celebration animation trigger */
  let showCelebration = $state(false);
  let hasShownCelebration = $state(false);

  async function downloadRecommendedModel() {
    setupState = 'downloading';
    downloadProgress = 0;
    downloadError = null;

    try {
      // Listen for progress events
      const progressUn = await listen<{ percentage: number }>('model-download-progress', (event) => {
        downloadProgress = event.payload.percentage;
      });
      downloadUnlisteners.push(progressUn);

      const completeUn = await listen<string>('model-download-complete', async () => {
        setupState = 'initialising';
        try {
          const modelDir = await invoke<string>('get_model_directory');
          await invoke('init_transcription', { modelPath: modelDir });
          transcriptionReady = true;
          setupState = 'ready';
        } catch (e) {
          downloadError = e instanceof Error ? e.message : String(e);
          setupState = 'error';
        }
        cleanupDownloadListeners();
      });
      downloadUnlisteners.push(completeUn);

      const errorUn = await listen<string>('model-download-error', (event) => {
        downloadError = event.payload;
        setupState = 'error';
        cleanupDownloadListeners();
      });
      downloadUnlisteners.push(errorUn);

      // Start the download (null = recommended model)
      await invoke('download_model');
    } catch (e) {
      downloadError = e instanceof Error ? e.message : String(e);
      setupState = 'error';
      cleanupDownloadListeners();
    }
  }

  function cleanupDownloadListeners() {
    for (const unlisten of downloadUnlisteners) {
      unlisten();
    }
    downloadUnlisteners = [];
  }

  function retryDownload() {
    invoke('reset_download_state').catch(() => {});
    downloadRecommendedModel();
  }

  $effect(() => {
    if (allRequiredDone && !hasShownCelebration && !isLoading) {
      showCelebration = true;
      hasShownCelebration = true;
      setTimeout(() => { showCelebration = false; }, 3000);
    }
  });

  onDestroy(() => {
    stopPermissionPoll();
    permissionChangedUnlisten?.();
    cleanupDownloadListeners();
  });

  onMount(async () => {
    loadAutostartState();
    loadDockState();
    getVersion().then((v) => { currentVersion = v; }).catch(() => {});

    // Listen for permission-changed events from the backend (microphone
    // dialog completion handler fires this for an immediate update).
    listen<string>('permission-changed', () => {
      checkPermissions();
    }).then((unlisten) => { permissionChangedUnlisten = unlisten; });

    await checkPermissions();

    if (allPermissionsGranted) {
      invoke('refresh_tray_menu').catch(() => {});
    } else {
      // Start adaptive polling — the standard macOS pattern for detecting
      // TCC changes. Stops automatically when all permissions are granted.
      startPermissionPoll();
    }
    const [statsResult, readyResult, downloadedResult, gpuResult] = await Promise.allSettled([
      invoke<TranscriptionStats>('get_transcription_stats_cmd'),
      invoke<boolean>('is_transcription_ready'),
      invoke<boolean>('check_model_downloaded', { modelId: null }),
      invoke<GpuInfo>('get_gpu_info'),
    ]);

    if (statsResult.status === 'fulfilled') {
      stats = statsResult.value;
    }

    if (readyResult.status === 'fulfilled') {
      transcriptionReady = readyResult.value;
    }

    if (downloadedResult.status === 'fulfilled') {
      modelDownloaded = downloadedResult.value;
    }

    if (gpuResult.status === 'fulfilled') {
      gpuInfo = gpuResult.value;
    }

    setupState = transcriptionReady ? 'ready' : 'needed';
    isLoading = false;

    // Ollama check runs separately to avoid blocking (30s timeout)
    if (!configStore.enhancement.enabled) {
      ollamaStatus = 'not-configured';
    } else {
      invoke<boolean>('check_ollama_available')
        .then((available) => {
          ollamaStatus = available ? 'connected' : 'unavailable';
        })
        .catch(() => {
          ollamaStatus = 'unavailable';
        });
    }
  });
</script>

{#if isLoading}
  <div class="loading">Loading...</div>
{:else if stats && stats.totalCount === 0}
  <!-- First-run setup: stepped checklist -->
  <div class="empty-state" class:celebrating={showCelebration}>
    <div class="empty-icon">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path
          d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
        <path
          d="M19 10v2a7 7 0 0 1-14 0v-2M12 19v4M8 23h8"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </div>
    {#if allRequiredDone}
      <p class="empty-title">Ready to go</p>
      <p class="empty-hint">Press your shortcut key to start recording.</p>
    {:else}
      <p class="empty-title">Set up Thoth</p>
      <p class="empty-hint">Three quick steps and you're recording.</p>
    {/if}
  </div>

  <!-- Setup steps -->
  <div class="setup-checklist">
    <!-- Step 1: Download speech model -->
    <div class="setup-step" class:completed={modelStepDone}>
      <div class="step-indicator" class:pending={!modelStepDone} class:done={modelStepDone}>
        {#if modelStepDone}
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M3 8.5l3.5 3.5 6.5-7" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        {:else}
          1
        {/if}
      </div>
      <div class="step-body">
        <p class="step-title">Download speech model</p>
        {#if modelStepDone}
          <p class="step-description done">Model ready</p>
        {:else}
          <p class="step-description">
            {#if setupState === 'downloading'}
              Downloading... {Math.round(downloadProgress)}%
            {:else if setupState === 'initialising'}
              Preparing transcription engine...
            {:else if setupState === 'error'}
              {downloadError ?? 'Download failed.'}
            {:else}
              A ~1.5 GB model that runs entirely on your machine. Nothing is sent to the cloud.
            {/if}
          </p>
          {#if setupState === 'downloading'}
            <div class="progress-bar">
              <div class="progress-fill" style="width: {Math.round(downloadProgress)}%"></div>
            </div>
          {:else if setupState === 'initialising'}
            <div class="progress-bar">
              <div class="progress-fill indeterminate"></div>
            </div>
          {/if}
          {#if setupState === 'needed'}
            <div class="step-actions">
              <button class="btn-setup" onclick={downloadRecommendedModel}>
                Download Recommended Model
              </button>
              <button class="btn-setup-alt" onclick={() => onNavigate('models')}>
                Choose a different model
              </button>
            </div>
          {:else if setupState === 'error'}
            <div class="step-actions">
              <button class="btn-setup" onclick={retryDownload}>
                Retry Download
              </button>
            </div>
          {/if}
        {/if}
      </div>
    </div>

    <!-- Step 2: Allow microphone -->
    <div class="setup-step" class:completed={micStepDone}>
      <div class="step-indicator" class:pending={!micStepDone} class:done={micStepDone}>
        {#if micStepDone}
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M3 8.5l3.5 3.5 6.5-7" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        {:else}
          2
        {/if}
      </div>
      <div class="step-body">
        <p class="step-title">Allow microphone access</p>
        {#if micStepDone}
          <p class="step-description done">Microphone access granted</p>
        {:else}
          <p class="step-description">Thoth needs to hear you to transcribe your speech.</p>
          <div class="step-actions">
            <button class="btn-setup" onclick={() => requestPermission('request_microphone_permission')}>Allow</button>
          </div>
        {/if}
      </div>
    </div>

    <!-- Step 3: Allow global shortcut -->
    <div class="setup-step" class:completed={shortcutStepDone} class:stale={accessibilityStale}>
      <div class="step-indicator" class:pending={!shortcutStepDone && !accessibilityStale} class:done={shortcutStepDone} class:warn={accessibilityStale}>
        {#if shortcutStepDone}
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M3 8.5l3.5 3.5 6.5-7" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        {:else if accessibilityStale}
          !
        {:else}
          3
        {/if}
      </div>
      <div class="step-body">
        <p class="step-title">Allow global shortcut</p>
        {#if shortcutStepDone}
          <p class="step-description done">Shortcut access granted</p>
        {:else if accessibilityStale}
          <p class="step-description stale-warning">Permission appears granted but isn't working. This can happen after app updates or reinstalls.</p>
          <div class="step-actions">
            <button class="btn-setup warning" onclick={() => resetPermissions(['Accessibility', 'ListenEvent'])} disabled={resettingPermissions}>
              {resettingPermissions ? 'Resetting...' : 'Reset & Re-grant'}
            </button>
          </div>
          {#if resetError}
            <p class="step-error">{resetError}</p>
          {/if}
        {:else}
          <p class="step-description">Lets you start recording from anywhere with a keyboard shortcut.</p>
          <div class="step-actions">
            <button class="btn-setup" onclick={() => requestPermission('request_accessibility')}>Allow</button>
          </div>
        {/if}
      </div>
    </div>
  </div>

  <!-- Optional settings -->
  <details class="optional-section">
    <summary class="optional-summary">Optional settings</summary>
    <div class="optional-content">
      <div class="status-row">
        <span
          class="status-dot"
          class:ready={inputMonitoringPermission === 'granted'}
          class:warning={inputMonitoringPermission === 'denied'}
        ></span>
        <span class="status-label">Input Monitoring</span>
        <span class="status-value">
          {#if inputMonitoringPermission === 'granted'}
            Granted
          {:else}
            <button class="btn-small" onclick={() => requestPermission('request_input_monitoring')}>Grant Access</button>
          {/if}
        </span>
      </div>
      {#if inputMonitoringPermission !== 'granted'}
        <p class="permission-hint">Needed only if you want to customise the recording shortcut</p>
      {/if}
      <div class="status-row">
        <span
          class="status-dot"
          class:ready={ollamaStatus === 'connected'}
          class:not-configured={ollamaStatus === 'not-configured'}
          class:warning={ollamaStatus === 'unavailable'}
          class:checking={ollamaStatus === 'checking'}
        ></span>
        <span class="status-label">AI Enhancement</span>
        <span class="status-value">
          {#if ollamaStatus === 'checking'}
            Checking...
          {:else if ollamaStatus === 'connected'}
            Connected
          {:else if ollamaStatus === 'not-configured'}
            Not configured
          {:else}
            Unavailable
          {/if}
        </span>
      </div>
      <div class="autostart-row">
        <span class="status-label">Launch at Login</span>
        <label class="toggle-switch">
          <input
            type="checkbox"
            checked={autostartEnabled}
            disabled={autostartLoading}
            onchange={handleAutostartToggle}
          />
          <span class="toggle-slider"></span>
        </label>
      </div>
      {#if autostartError}
        <div class="setting-error">{autostartError}</div>
      {/if}
      <div class="autostart-row">
        <span class="status-label">Show in Dock</span>
        <label class="toggle-switch">
          <input
            type="checkbox"
            checked={showInDock}
            disabled={dockLoading}
            onchange={handleDockToggle}
          />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>
  </details>
{:else if stats}
  <!-- Summary Cards -->
  <section class="settings-section">
    <div class="section-header">
      <h2 class="section-title">Summary</h2>
    </div>
    <div class="section-content">
      <div class="cards">
        <div class="card">
          <span class="card-value">{stats.totalCount}</span>
          <span class="card-label">Transcriptions</span>
        </div>
        <div class="card">
          <span class="card-value">{formatTotalDuration(stats.totalAudioDuration)}</span>
          <span class="card-label">Total audio</span>
        </div>
        <div class="card">
          <span class="card-value">{stats.enhancedCount}</span>
          <span class="card-label">Enhanced</span>
        </div>
        <div class="card">
          <span class="card-value">{avgRecordingDuration > 0 ? formatDuration(avgRecordingDuration) : '--'}</span>
          <span class="card-label">Avg recording</span>
        </div>
      </div>
    </div>
  </section>

  <!-- Updates -->
  <section class="settings-section">
    <div class="section-header">
      <h2 class="section-title">Updates</h2>
      <p class="section-description">Application version and update preferences</p>
    </div>
    <div class="section-content">
      <div class="status-list">
        <div class="status-row">
          <span class="status-label">Current Version</span>
          <span class="status-value">{currentVersion}</span>
        </div>
        <div class="status-row">
          <span class="status-label">Status</span>
          <span class="status-value">
            {#if updaterState.state === 'checking'}
              Checking...
            {:else if updaterState.state === 'available'}
              <span class="status-update-available">Update available: {updaterState.updateVersion}</span>
            {:else if updaterState.state === 'up-to-date'}
              Up to date
            {:else if updaterState.state === 'error'}
              <span class="status-error">{updaterState.error || 'Check failed'}</span>
            {:else}
              Not checked
            {/if}
          </span>
        </div>
        <div class="autostart-row">
          <span class="status-label">Check on Launch</span>
          <label class="toggle-switch">
            <input
              type="checkbox"
              bind:checked={configStore.general.checkForUpdates}
              onchange={async () => {
                await configStore.save();
              }}
            />
            <span class="toggle-slider"></span>
          </label>
        </div>
        <div class="autostart-row">
          <span class="status-label">
            {#if updaterState.state === 'checking'}
              Checking...
            {:else}
              Check Now
            {/if}
          </span>
          <button
            class="btn-small"
            onclick={() => checkForUpdate()}
            disabled={updaterState.state === 'checking'}
          >
            {updaterState.state === 'checking' ? 'Checking...' : 'Check for Updates'}
          </button>
        </div>
      </div>
    </div>
  </section>

  <!-- System Status -->
  <section class="settings-section">
    <div class="section-header">
      <h2 class="section-title">System</h2>
      <p class="section-description">Services, permissions, and application preferences</p>
    </div>
    <div class="section-content">
      <div class="status-list">
        <div class="status-row">
          <span
            class="status-dot"
            class:ready={transcriptionReady}
            class:not-configured={!modelDownloaded}
            class:checking={modelDownloaded && !transcriptionReady}
          ></span>
          <span class="status-label">Transcription</span>
          <span class="status-value">
            {#if transcriptionReady}
              Ready
            {:else if modelDownloaded}
              Loading...
            {:else}
              No model downloaded
            {/if}
          </span>
        </div>
        <div class="status-row">
          <span
            class="status-dot"
            class:ready={gpuInfo?.gpu_available}
            class:not-configured={!gpuInfo}
          ></span>
          <span class="status-label">GPU</span>
          <span class="status-value">
            {#if gpuInfo}
              {#if gpuInfo.gpu_available}
                <span class="gpu-info">
                  <span class="gpu-backend">{gpuInfo.compiled_backend}</span>
                  {#if gpuInfo.gpu_name}
                    <span class="gpu-name" title={gpuInfo.gpu_name}>{gpuInfo.gpu_name}</span>
                  {/if}
                  {#if gpuInfo.vram_mb}
                    <span class="gpu-vram">{gpuInfo.vram_mb} MB</span>
                  {/if}
                </span>
              {:else}
                <span class="gpu-cpu">CPU only</span>
              {/if}
            {:else}
              Checking...
            {/if}
          </span>
        </div>
        <div class="status-row">
          <span
            class="status-dot"
            class:ready={ollamaStatus === 'connected'}
            class:not-configured={ollamaStatus === 'not-configured'}
            class:warning={ollamaStatus === 'unavailable'}
            class:checking={ollamaStatus === 'checking'}
          ></span>
          <span class="status-label">Enhancement</span>
          <span class="status-value">
            {#if ollamaStatus === 'checking'}
              Checking...
            {:else if ollamaStatus === 'connected'}
              Connected
            {:else if ollamaStatus === 'not-configured'}
              Not configured
            {:else}
              Unavailable
            {/if}
          </span>
        </div>
        <div class="status-row">
          <span class="status-dot ready"></span>
          <span class="status-label">Microphone</span>
          <span class="status-value truncate">{deviceName}</span>
        </div>
        <div class="status-row">
          <span class="status-dot" class:ready={allPermissionsGranted} class:warning={!allPermissionsGranted}></span>
          <span class="status-label">Permissions</span>
          <span class="status-value">
            {#if allPermissionsGranted}
              All granted
            {:else}
              {[
                microphonePermission !== 'granted' ? 'Mic' : '',
                accessibilityPermission !== 'granted' ? 'Accessibility' : '',
                inputMonitoringPermission !== 'granted' ? 'Input Monitoring' : '',
              ].filter(Boolean).join(', ')} needed
            {/if}
          </span>
        </div>
        <div class="autostart-row">
          <span class="status-label">Launch at Login</span>
          <label class="toggle-switch">
            <input
              type="checkbox"
              checked={autostartEnabled}
              disabled={autostartLoading}
              onchange={handleAutostartToggle}
            />
            <span class="toggle-slider"></span>
          </label>
        </div>
        {#if autostartError}
          <div class="setting-error">{autostartError}</div>
        {/if}
        <div class="autostart-row">
          <span class="status-label">Show in Dock</span>
          <label class="toggle-switch">
            <input
              type="checkbox"
              checked={showInDock}
              disabled={dockLoading}
              onchange={handleDockToggle}
            />
            <span class="toggle-slider"></span>
          </label>
        </div>
      </div>
    </div>
  </section>

  <!-- Troubleshooting (advanced) -->
  <details class="optional-section">
    <summary class="optional-summary">Troubleshooting</summary>
    <div class="optional-content">
      <p class="troubleshoot-description">
        After installing a new version of Thoth, macOS may block it or hold onto stale permission entries.
        Run these steps in order — each one takes about 2 seconds.
      </p>

      <!-- Step 1: Quarantine -->
      <div class="fix-step">
        <div class="fix-step-header">
          <span class="fix-step-num">1</span>
          <div class="fix-step-info">
            <span class="fix-step-title">Remove quarantine flag</span>
            <span class="fix-step-desc">macOS marks downloaded apps as quarantined. Removes the "are you sure?" block.</span>
          </div>
          <button
            class="btn-small"
            onclick={async () => {
              try {
                await invoke('remove_quarantine');
                permFixStatus.quarantine = 'done';
              } catch (e) {
                permFixStatus.quarantine = 'error';
              }
            }}
          >
            {permFixStatus.quarantine === 'done' ? '✓ Done' : permFixStatus.quarantine === 'error' ? '✗ Error' : 'Fix'}
          </button>
        </div>
      </div>

      <!-- Step 2: Reset all TCC -->
      <div class="fix-step">
        <div class="fix-step-header">
          <span class="fix-step-num">2</span>
          <div class="fix-step-info">
            <span class="fix-step-title">Reset system permissions</span>
            <span class="fix-step-desc">Clears stale grants for Input Monitoring, Accessibility, and Microphone. macOS will re-prompt for each one.</span>
          </div>
          <button
            class="btn-small warning"
            onclick={async () => {
              try {
                await invoke('reset_tcc_permissions', { services: ['ListenEvent', 'Accessibility', 'Microphone'] });
                permFixStatus.tcc = 'done';
              } catch (e) {
                permFixStatus.tcc = 'error';
              }
            }}
          >
            {permFixStatus.tcc === 'done' ? '✓ Done' : permFixStatus.tcc === 'error' ? '✗ Error' : 'Reset'}
          </button>
        </div>
      </div>

      <!-- Step 3: Open privacy panes -->
      <div class="fix-step">
        <div class="fix-step-header">
          <span class="fix-step-num">3</span>
          <div class="fix-step-info">
            <span class="fix-step-title">Open Privacy &amp; Security</span>
            <span class="fix-step-desc">Opens the two panels where you'll re-grant permissions after relaunching Thoth.</span>
          </div>
          <div style="display:flex;gap:4px">
            <button class="btn-small" onclick={() => invoke('open_privacy_pane', { pane: 'accessibility' })}>
              Accessibility
            </button>
            <button class="btn-small" onclick={() => invoke('open_privacy_pane', { pane: 'input-monitoring' })}>
              Input Monitoring
            </button>
          </div>
        </div>
      </div>

      <!-- Step 4: Quit and relaunch -->
      <div class="fix-step">
        <div class="fix-step-header">
          <span class="fix-step-num">4</span>
          <div class="fix-step-info">
            <span class="fix-step-title">Quit &amp; relaunch Thoth</span>
            <span class="fix-step-desc">Restart the app so macOS re-prompts for each permission. Click Allow in each dialog.</span>
          </div>
          <button
            class="btn-small"
            onclick={async () => {
              await invoke('relaunch_app').catch(() => {});
            }}
          >
            Relaunch
          </button>
        </div>
      </div>

      {#if resetError}
        <p class="step-error">{resetError}</p>
      {/if}

      <details class="manual-fix-details">
        <summary class="manual-fix-summary">Manual fix (Terminal)</summary>
        <div class="manual-fix">
          <code class="manual-fix-code">xattr -dr com.apple.quarantine /Applications/Thoth.app</code>
          <code class="manual-fix-code">tccutil reset ListenEvent com.poodle64.thoth</code>
          <code class="manual-fix-code">tccutil reset Accessibility com.poodle64.thoth</code>
          <code class="manual-fix-code">tccutil reset Microphone com.poodle64.thoth</code>
          <p class="manual-fix-hint">Then restart Thoth and re-grant permissions in System Settings → Privacy &amp; Security.</p>
        </div>
      </details>
    </div>
  </details>
{/if}

<style>
  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--spacing-xl);
    color: var(--color-text-tertiary);
    font-size: var(--text-sm);
  }

  /* Empty state */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 32px 24px;
    text-align: center;
  }

  .empty-icon {
    width: 48px;
    height: 48px;
    color: var(--color-text-tertiary);
    margin-bottom: 16px;
    opacity: 0.5;
  }

  .empty-icon svg {
    width: 100%;
    height: 100%;
  }

  .empty-title {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .empty-hint {
    margin: 6px 0 0;
    font-size: var(--text-sm);
    color: var(--color-text-tertiary);
  }

  /* Setup checklist */
  .setup-checklist {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .setup-step {
    display: flex;
    gap: 14px;
    padding: 16px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    transition: border-color var(--transition-normal);
  }

  .setup-step.completed {
    border-color: color-mix(in srgb, var(--color-success) 30%, var(--color-border-subtle));
  }

  .step-indicator {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    font-size: var(--text-sm);
    font-weight: 600;
  }

  .step-indicator svg {
    width: 14px;
    height: 14px;
  }

  .step-indicator.pending {
    background: var(--color-bg-tertiary);
    color: var(--color-text-secondary);
  }

  .step-indicator.done {
    background: color-mix(in srgb, var(--color-success) 15%, var(--color-bg-secondary));
    color: var(--color-success);
  }

  .step-body {
    flex: 1;
    min-width: 0;
  }

  .step-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-primary);
    margin: 0 0 4px 0;
  }

  .step-description {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0 0 12px 0;
    line-height: 1.4;
  }

  .step-description.done {
    color: var(--color-text-tertiary);
    margin-bottom: 0;
  }

  /* Celebration */
  .empty-state.celebrating .empty-title {
    animation: celebrateText 0.6s ease-out;
  }

  @keyframes celebrateText {
    0% { transform: scale(1); }
    50% { transform: scale(1.05); color: var(--color-success); }
    100% { transform: scale(1); }
  }

  /* Optional settings */
  .optional-section {
    margin-top: 8px;
  }

  .optional-summary {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    cursor: pointer;
    padding: 8px 0;
    list-style: none;
  }

  .optional-summary::before {
    content: '>';
    display: inline-block;
    margin-right: 6px;
    transition: transform var(--transition-fast);
  }

  details[open] > .optional-summary::before {
    transform: rotate(90deg);
  }

  .optional-content {
    padding: 8px 14px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .progress-bar {
    height: 6px;
    background: var(--color-bg-tertiary);
    border-radius: var(--radius-full);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--color-accent);
    border-radius: var(--radius-full);
    transition: width 0.3s ease;
  }

  .progress-fill.indeterminate {
    width: 40%;
    animation: indeterminate 1.2s ease-in-out infinite;
  }

  @keyframes indeterminate {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(350%); }
  }

  .step-actions {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .btn-setup {
    padding: 8px 16px;
    font-size: var(--text-sm);
    font-weight: 500;
    background: var(--color-accent);
    color: white;
    border: none;
    border-radius: var(--radius-md);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .btn-setup:hover {
    background: var(--color-accent-hover);
  }

  .btn-setup-alt {
    padding: 8px 12px;
    font-size: var(--text-sm);
    background: none;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    transition: color var(--transition-fast);
  }

  .btn-setup-alt:hover {
    color: var(--color-text-primary);
  }

  /* Summary cards */
  .cards {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 10px;
  }

  .card {
    display: flex;
    flex-direction: column;
    padding: 14px 16px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
  }

  .card-value {
    font-size: 22px;
    font-weight: 600;
    color: var(--color-text-primary);
    font-variant-numeric: tabular-nums;
    line-height: 1.2;
  }

  .card-label {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
    margin-top: 4px;
  }

  /* Truncation */
  .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* System status */
  .status-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 14px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
  }

  .status-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--color-text-tertiary);
  }

  .status-dot.ready {
    background: var(--color-success);
  }

  .status-dot.not-configured {
    background: var(--color-text-tertiary);
  }

  .status-dot.warning {
    background: var(--color-warning);
  }

  .status-dot.checking {
    background: var(--color-text-tertiary);
    animation: pulse 1s ease-in-out infinite;
  }

  .status-label {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    width: 110px;
    flex-shrink: 0;
  }

  .status-value {
    font-size: var(--text-sm);
    color: var(--color-text-primary);
    font-weight: 500;
    min-width: 0;
  }

  .permission-hint {
    margin: -2px 0 4px 18px;
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
    line-height: 1.3;
  }

  .autostart-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 0;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.4;
    }
  }

  /* GPU info display */
  .gpu-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .gpu-backend {
    font-weight: 600;
    color: var(--color-success);
  }

  .gpu-name {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .gpu-vram {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  .gpu-cpu {
    color: var(--color-text-tertiary);
  }

  .step-indicator.warn {
    background: color-mix(in srgb, var(--color-warning) 15%, var(--color-bg-secondary));
    color: var(--color-warning);
    font-weight: 700;
  }

  .setup-step.stale {
    border-color: color-mix(in srgb, var(--color-warning) 40%, var(--color-border-subtle));
  }

  .stale-warning {
    color: var(--color-warning) !important;
  }

  .btn-setup.warning,
  .btn-small.warning {
    background: var(--color-warning);
    color: var(--color-bg-primary, #1a1a1a);
  }

  .btn-setup.warning:hover,
  .btn-small.warning:hover {
    background: color-mix(in srgb, var(--color-warning) 85%, black);
  }

  .step-error {
    margin: 6px 0 0;
    font-size: var(--text-xs);
    color: var(--color-error, #ef4444);
  }

  /* Manual fix */
  .manual-fix {
    margin-top: 8px;
    padding: 10px 12px;
    background: var(--color-bg-tertiary);
    border-radius: var(--radius-sm);
  }

  .manual-fix-code {
    display: block;
    margin-bottom: 4px;
    padding: 8px 10px;
    background: var(--color-bg-primary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono, 'SF Mono', 'Fira Code', monospace);
    font-size: 11px;
    color: var(--color-text-primary);
    word-break: break-all;
    user-select: all;
    cursor: text;
  }

  .manual-fix-hint {
    margin: 6px 0 0;
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  .manual-fix-details {
    margin-top: 8px;
  }

  .manual-fix-summary {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
    cursor: pointer;
  }

  /* Troubleshooting section */
  .troubleshoot-description {
    margin: 0 0 12px;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.4;
  }

  .fix-step {
    background: var(--color-bg-tertiary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: 10px 12px;
    margin-bottom: 8px;
  }

  .fix-step-header {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .fix-step-num {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--color-accent);
    color: var(--color-bg);
    font-size: 11px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .fix-step-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .fix-step-title {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text-primary);
  }

  .fix-step-desc {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    line-height: 1.3;
  }

</style>
