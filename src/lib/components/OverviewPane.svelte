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
	import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart';
	import { configStore } from '../stores/config.svelte';
	import { settingsStore } from '../stores/settings.svelte';
	import {
		formatDuration,
		formatTotalDuration,
		formatSpeedFactor
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

  interface Props {
    /** Callback to navigate to another Settings pane */
    onNavigate: (paneId: string) => void;
  }

	let { onNavigate }: Props = $props();

	let stats = $state<TranscriptionStats | null>(null);
	let transcriptionReady = $state(false);
	let isLoading = $state(true);
	let ollamaStatus = $state<'checking' | 'connected' | 'unavailable' | 'not-configured'>(
		'checking'
	);

	const updaterState = getUpdaterState();
	let currentVersion = $state('2026.2.2'); // Will be read from package.json or tauri config

  /** Average speed factor across all transcription models */
  let avgSpeedFactor = $derived.by(() => {
    if (!stats || stats.transcriptionModels.length === 0) return 0;
    const totalWeighted = stats.transcriptionModels.reduce(
      (sum, m) => sum + m.speedFactor * m.count,
      0
    );
    const totalCount = stats.transcriptionModels.reduce((sum, m) => sum + m.count, 0);
    return totalCount > 0 ? totalWeighted / totalCount : 0;
  });

  /** Top models by usage, capped at 3 */
  let topModels = $derived.by(() => {
    if (!stats) return [];
    const sorted = [...stats.transcriptionModels].sort((a, b) => b.count - a.count);
    return sorted.slice(0, 3);
  });

  let remainingModelCount = $derived(
    stats ? Math.max(0, stats.transcriptionModels.length - 3) : 0
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
  let microphonePermission = $state<'unknown' | 'granted' | 'denied'>('unknown');
  let accessibilityPermission = $state<'unknown' | 'granted' | 'denied'>('unknown');
  let inputMonitoringPermission = $state<'unknown' | 'granted' | 'denied'>('unknown');

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

  let permissionPollTimer: ReturnType<typeof setInterval> | null = null;
  let permissionPollCount = 0;
  const POLL_INTERVAL_MS = 2000;
  const MAX_POLL_ATTEMPTS = 15; // 30 seconds total

  async function checkPermissions() {
    try {
      const micStatus = await invoke<string>('check_microphone_permission');
      microphonePermission = micStatus === 'granted' ? 'granted' : 'denied';
    } catch (error) {
      console.error('Failed to check microphone permission:', error);
      microphonePermission = 'unknown';
    }

    try {
      const accessStatus = await invoke<boolean>('check_accessibility');
      accessibilityPermission = accessStatus ? 'granted' : 'denied';
    } catch (error) {
      console.error('Failed to check accessibility:', error);
      accessibilityPermission = 'unknown';
    }

    try {
      const inputStatus = await invoke<boolean>('check_input_monitoring');
      inputMonitoringPermission = inputStatus ? 'granted' : 'denied';
    } catch (error) {
      console.error('Failed to check input monitoring:', error);
      inputMonitoringPermission = 'unknown';
    }
  }

  function stopPermissionPolling() {
    if (permissionPollTimer !== null) {
      clearInterval(permissionPollTimer);
      permissionPollTimer = null;
      permissionPollCount = 0;
    }
  }

  function startPermissionPolling() {
    // Don't start if already polling
    if (permissionPollTimer !== null) return;

    permissionPollCount = 0;
    permissionPollTimer = setInterval(async () => {
      permissionPollCount++;
      await checkPermissions();

      // Stop if all permissions are granted or we've exceeded attempts
      const allGranted =
        microphonePermission === 'granted' &&
        accessibilityPermission === 'granted' &&
        inputMonitoringPermission === 'granted';

      if (allGranted) {
        stopPermissionPolling();
        // Refresh tray menu so status shows "Ready" instead of stale permission warning
        invoke('refresh_tray_menu').catch(() => {});
      } else if (permissionPollCount >= MAX_POLL_ATTEMPTS) {
        stopPermissionPolling();
      }
    }, POLL_INTERVAL_MS);
  }

  /** Request a permission and start auto-polling for status changes */
  function requestPermission(command: string) {
    invoke(command);
    startPermissionPolling();
  }

  /** Model download state for setup card */
  type SetupState = 'needed' | 'downloading' | 'initialising' | 'ready' | 'error';
  let setupState = $state<SetupState>('ready');
  let downloadProgress = $state(0);
  let downloadError = $state<string | null>(null);
  let downloadUnlisteners: UnlistenFn[] = [];

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

  onDestroy(() => {
    stopPermissionPolling();
    cleanupDownloadListeners();
  });

  onMount(async () => {
    loadAutostartState();
    loadDockState();
    await checkPermissions();

    // If all permissions are already granted, refresh tray in case it was built
    // before permissions were available (e.g. after TCC reset + reinstall)
    if (
      microphonePermission === 'granted' &&
      accessibilityPermission === 'granted' &&
      inputMonitoringPermission === 'granted'
    ) {
      invoke('refresh_tray_menu').catch(() => {});
    }
    const [statsResult, readyResult] = await Promise.allSettled([
      invoke<TranscriptionStats>('get_transcription_stats_cmd'),
      invoke<boolean>('is_transcription_ready'),
    ]);

    if (statsResult.status === 'fulfilled') {
      stats = statsResult.value;
    }

    if (readyResult.status === 'fulfilled') {
      transcriptionReady = readyResult.value;
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

<div class="overview-pane">
  {#if isLoading}
    <div class="loading">Loading...</div>
  {:else if stats && stats.totalCount === 0}
    <!-- Empty state for fresh install -->
    <div class="empty-state">
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
      {#if setupState === 'ready'}
        <p class="empty-title">Ready to transcribe</p>
        <p class="empty-hint">Press your shortcut key to start recording.</p>
      {:else}
        <p class="empty-title">Welcome to Thoth</p>
        <p class="empty-hint">Download a transcription model to get started.</p>
      {/if}
    </div>

    <!-- Setup card: model download -->
    {#if setupState !== 'ready'}
      <section class="setup-card" aria-label="Get started">
        <div class="setup-header">
          <svg class="setup-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" stroke-linecap="round" stroke-linejoin="round" />
            <polyline points="7 10 12 15 17 10" stroke-linecap="round" stroke-linejoin="round" />
            <line x1="12" y1="15" x2="12" y2="3" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
          <div class="setup-text">
            <p class="setup-title">Get Started</p>
            <p class="setup-description">
              {#if setupState === 'needed'}
                Download the recommended transcription model (~1.5 GB). This runs entirely on your machine.
              {:else if setupState === 'downloading'}
                Downloading model... {Math.round(downloadProgress)}%
              {:else if setupState === 'initialising'}
                Preparing transcription engine...
              {:else if setupState === 'error'}
                {downloadError ?? 'Download failed.'}
              {/if}
            </p>
          </div>
        </div>

        {#if setupState === 'downloading'}
          <div class="progress-bar">
            <div class="progress-fill" style="width: {Math.round(downloadProgress)}%"></div>
          </div>
        {:else if setupState === 'initialising'}
          <div class="progress-bar">
            <div class="progress-fill indeterminate"></div>
          </div>
        {/if}

        <div class="setup-actions">
          {#if setupState === 'needed'}
            <button class="btn-setup" onclick={downloadRecommendedModel}>
              Download Recommended Model
            </button>
            <button class="btn-setup-alt" onclick={() => onNavigate('models')}>
              Choose a different model
            </button>
          {:else if setupState === 'error'}
            <button class="btn-setup" onclick={retryDownload}>
              Retry Download
            </button>
          {/if}
        </div>
      </section>
    {/if}

    <!-- System status even on fresh install -->
    <section class="section" aria-labelledby="system-title-empty">
      <h3 id="system-title-empty" class="section-title">System</h3>
      <div class="status-list">
        <div class="status-row">
          <span
            class="status-dot"
            class:ready={transcriptionReady}
            class:not-configured={!transcriptionReady}
          ></span>
          <span class="status-label">Transcription</span>
          <span class="status-value">
            {transcriptionReady ? 'Ready' : 'No model downloaded'}
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
          <span
            class="status-dot"
            class:ready={microphonePermission === 'granted'}
            class:warning={microphonePermission === 'denied'}
          ></span>
          <span class="status-label">Mic Permission</span>
          <span class="status-value">
            {#if microphonePermission === 'granted'}
              Granted
            {:else}
              <span class="permission-actions">
                <button class="btn-small" onclick={() => requestPermission('request_microphone_permission')}>Grant Access</button>
                <button class="btn-icon" onclick={checkPermissions} title="Refresh">&#8635;</button>
              </span>
            {/if}
          </span>
        </div>
        <div class="status-row">
          <span
            class="status-dot"
            class:ready={accessibilityPermission === 'granted'}
            class:warning={accessibilityPermission === 'denied'}
          ></span>
          <span class="status-label">Accessibility</span>
          <span class="status-value">
            {#if accessibilityPermission === 'granted'}
              Granted
            {:else}
              <span class="permission-actions">
                <button class="btn-small" onclick={() => requestPermission('request_accessibility')}>Grant Access</button>
                <button class="btn-icon" onclick={checkPermissions} title="Refresh">&#8635;</button>
              </span>
            {/if}
          </span>
        </div>
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
              <span class="permission-actions">
                <button class="btn-small" onclick={() => requestPermission('request_input_monitoring')}>Grant Access</button>
                <button class="btn-icon" onclick={checkPermissions} title="Refresh">&#8635;</button>
              </span>
            {/if}
          </span>
        </div>
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
    </section>
  {:else if stats}
    <!-- Summary Cards -->
    <section class="section" aria-labelledby="summary-title">
      <h3 id="summary-title" class="section-title">Summary</h3>
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
          <span class="card-value">{avgSpeedFactor > 0 ? formatSpeedFactor(avgSpeedFactor) : '--'}</span>
          <span class="card-label">Avg RTFX</span>
        </div>
      </div>
    </section>

    <!-- Model Performance -->
    {#if topModels.length > 0}
      <section class="section" aria-labelledby="models-title">
        <h3 id="models-title" class="section-title">Models</h3>
        <div class="model-list">
          {#each topModels as model}
            <div class="model-row">
              <div class="model-name truncate">{model.name}</div>
              <div class="model-metrics">
                <span class="metric">
                  <span class="metric-value">{model.count}</span>
                  <span class="metric-label">uses</span>
                </span>
                <span class="metric">
                  <span class="metric-value">{formatSpeedFactor(model.speedFactor)}</span>
                  <span class="metric-label">RTFX</span>
                </span>
                <span class="metric">
                  <span class="metric-value">{formatDuration(model.avgProcessingTime)}</span>
                  <span class="metric-label">avg time</span>
                </span>
              </div>
            </div>
          {/each}
          {#if remainingModelCount > 0}
            <button class="link-btn" onclick={() => onNavigate('history')}>
              and {remainingModelCount} more
            </button>
          {/if}
        </div>
			</section>
		{/if}

		<!-- Updates -->
		<section class="section" aria-labelledby="updates-title">
			<h3 id="updates-title" class="section-title">Updates</h3>
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
				<div class="status-row">
					<button
						class="btn-check-update"
						onclick={() => checkForUpdate()}
						disabled={updaterState.state === 'checking'}
					>
						{updaterState.state === 'checking' ? 'Checking...' : 'Check for Updates'}
					</button>
				</div>
				<div class="status-row">
					<label class="toggle-row">
						<input
							type="checkbox"
							bind:checked={configStore.general.checkForUpdates}
							onchange={async () => {
								await configStore.save();
							}}
						/>
						<span>Automatically check for updates on launch</span>
					</label>
				</div>
			</div>
		</section>

		<!-- System Status -->
		<section class="section" aria-labelledby="system-title">
			<h3 id="system-title" class="section-title">System</h3>
      <div class="status-list">
        <div class="status-row">
          <span
            class="status-dot"
            class:ready={transcriptionReady}
            class:not-configured={!transcriptionReady}
          ></span>
          <span class="status-label">Transcription</span>
          <span class="status-value">
            {transcriptionReady ? 'Ready' : 'No model downloaded'}
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
          <span
            class="status-dot"
            class:ready={microphonePermission === 'granted'}
            class:warning={microphonePermission === 'denied'}
          ></span>
          <span class="status-label">Mic Permission</span>
          <span class="status-value">
            {#if microphonePermission === 'granted'}
              Granted
            {:else}
              <span class="permission-actions">
                <button class="btn-small" onclick={() => requestPermission('request_microphone_permission')}>Grant Access</button>
                <button class="btn-icon" onclick={checkPermissions} title="Refresh">&#8635;</button>
              </span>
            {/if}
          </span>
        </div>
        <div class="status-row">
          <span
            class="status-dot"
            class:ready={accessibilityPermission === 'granted'}
            class:warning={accessibilityPermission === 'denied'}
          ></span>
          <span class="status-label">Accessibility</span>
          <span class="status-value">
            {#if accessibilityPermission === 'granted'}
              Granted
            {:else}
              <span class="permission-actions">
                <button class="btn-small" onclick={() => requestPermission('request_accessibility')}>Grant Access</button>
                <button class="btn-icon" onclick={checkPermissions} title="Refresh">&#8635;</button>
              </span>
            {/if}
          </span>
        </div>
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
              <span class="permission-actions">
                <button class="btn-small" onclick={() => requestPermission('request_input_monitoring')}>Grant Access</button>
                <button class="btn-icon" onclick={checkPermissions} title="Refresh">&#8635;</button>
              </span>
            {/if}
          </span>
        </div>
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
    </section>
  {/if}
</div>

<style>
  .overview-pane {
    display: flex;
    flex-direction: column;
    gap: 24px;
    padding: 24px 32px 48px 32px;
    overflow-y: auto;
    height: 100%;
  }

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

  /* Setup card */
  .setup-card {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 18px;
    background: color-mix(in srgb, var(--color-accent) 8%, var(--color-bg-secondary));
    border: 1px solid color-mix(in srgb, var(--color-accent) 25%, var(--color-border-subtle));
    border-radius: var(--radius-md);
  }

  .setup-header {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }

  .setup-icon {
    width: 24px;
    height: 24px;
    flex-shrink: 0;
    color: var(--color-accent);
    margin-top: 1px;
  }

  .setup-text {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .setup-title {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .setup-description {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.4;
  }

  .progress-bar {
    height: 4px;
    background: var(--color-bg-tertiary);
    border-radius: 2px;
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--color-accent);
    border-radius: 2px;
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

  .setup-actions {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .btn-setup {
    padding: 8px 16px;
    font-size: var(--text-sm);
    font-weight: 500;
    background: var(--color-accent);
    color: #000;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }

  .btn-setup:hover {
    opacity: 0.85;
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

  /* Sections */
  .section {
    display: flex;
    flex-direction: column;
  }

  .section-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--color-text-tertiary);
    margin: 0 0 10px 0;
    text-transform: uppercase;
    letter-spacing: 0.05em;
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

  /* Model performance */
  .model-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .model-row {
    padding: 10px 14px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
  }

  .model-name {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text-primary);
    margin-bottom: 6px;
    font-family: var(--font-mono, monospace);
  }

  .model-metrics {
    display: flex;
    gap: var(--spacing-lg);
    flex-wrap: wrap;
  }

  .metric {
    display: flex;
    align-items: baseline;
    gap: 4px;
  }

  .metric-value {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-accent);
    font-variant-numeric: tabular-nums;
  }

  .metric-label {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  /* Truncation */
  .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Link button */
  .link-btn {
    background: none;
    border: none;
    padding: 6px 0;
    font-size: var(--text-xs);
    color: var(--color-accent);
    cursor: pointer;
    text-align: left;
    font-weight: 500;
  }

  .link-btn:hover {
    text-decoration: underline;
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

  .permission-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .autostart-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 14px;
    margin-top: 8px;
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
</style>
