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
  import { formatDuration, formatTotalDuration } from '../utils/format';
  import { getUpdaterState, checkForUpdate } from '../stores/updater.svelte';
  import { Button } from '$components/ui/button';
  import { Switch } from '$components/ui/switch';
  import { Badge } from '$components/ui/badge';
  import * as Card from '$components/ui/card';
  import * as AlertDialog from '$components/ui/alert-dialog';
  import * as Alert from '$components/ui/alert';

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
    stats && stats.analysableCount > 0 ? stats.totalAudioDuration / stats.analysableCount : 0
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

  /** Stale accessibility reset state */
  let resettingAccessibility = $state(false);
  let resetAccessibilityError = $state<string | null>(null);
  let resetAccessibilityMessage = $state<string | null>(null);

  let permFixStatus = $state<{ quarantine: string; tcc: string }>({ quarantine: '', tcc: '' });

  async function handleAutostartToggle(checked: boolean) {
    autostartLoading = true;
    autostartError = null;

    try {
      if (checked) {
        await enable();
        autostartEnabled = true;
      } else {
        await disable();
        autostartEnabled = false;
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

  async function handleDockToggle(checked: boolean) {
    dockLoading = true;
    try {
      await invoke('set_show_in_dock', { show: checked });
      showInDock = checked;
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
   * Reset the stale Accessibility TCC entry and guide the user to re-grant.
   *
   * Stale entries arise after app rebuilds or reinstalls: System Settings shows
   * the toggle on, but AXIsProcessTrusted() returns false. tccutil reset clears
   * the entry so macOS will prompt again on next launch.
   */
  async function handleResetAccessibility() {
    resettingAccessibility = true;
    resetAccessibilityError = null;
    resetAccessibilityMessage = null;

    try {
      const message = await invoke<string>('reset_tcc_permissions', {
        services: ['Accessibility', 'ListenEvent'],
      });
      resetAccessibilityMessage = message;
      await checkPermissions();
      // Guide the user to re-grant via System Settings
      invoke('open_privacy_pane', { pane: 'accessibility' }).catch(() => {});
    } catch (error) {
      resetAccessibilityError = error instanceof Error ? error.message : String(error);
      console.error('Failed to reset accessibility permission:', error);
    } finally {
      resettingAccessibility = false;
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
      const progressUn = await listen<{ percentage: number }>(
        'model-download-progress',
        (event) => {
          downloadProgress = event.payload.percentage;
        }
      );
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
      setTimeout(() => {
        showCelebration = false;
      }, 3000);
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
    getVersion()
      .then((v) => {
        currentVersion = v;
      })
      .catch(() => {});

    // Listen for permission-changed events from the backend (microphone
    // dialog completion handler fires this for an immediate update).
    listen<string>('permission-changed', () => {
      checkPermissions();
    }).then((unlisten) => {
      permissionChangedUnlisten = unlisten;
    });

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
  <div class="flex items-center justify-center p-8 text-muted-foreground text-sm">Loading...</div>
{:else if stats && stats.totalCount === 0}
  <!-- First-run setup: stepped checklist -->
  <div
    class="flex flex-col items-center px-6 pt-8 pb-4 text-center"
    class:celebrating={showCelebration}
  >
    <div class="w-12 h-12 text-muted-foreground mb-4 opacity-50">
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
      <p class="m-0 text-lg font-semibold text-foreground">Ready to go</p>
      <p class="mt-1.5 text-sm text-muted-foreground">
        Press your shortcut key to start recording.
      </p>
    {:else}
      <p class="m-0 text-lg font-semibold text-foreground">Set up Thoth</p>
      <p class="mt-1.5 text-sm text-muted-foreground">Three quick steps and you're recording.</p>
    {/if}
  </div>

  <!-- Setup steps -->
  <div class="flex flex-col gap-3">
    <!-- Step 1: Download speech model -->
    <div
      class={[
        'flex gap-3.5 p-4 bg-secondary border rounded-md transition-colors',
        modelStepDone && 'border-green-500/30',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <div
        class={[
          'w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0 text-sm font-semibold',
          !modelStepDone && 'bg-muted text-muted-foreground',
          modelStepDone && 'bg-green-500/15 text-green-600',
        ]
          .filter(Boolean)
          .join(' ')}
      >
        {#if modelStepDone}
          <svg
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="w-3.5 h-3.5"
          >
            <path d="M3 8.5l3.5 3.5 6.5-7" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        {:else}
          1
        {/if}
      </div>
      <div class="flex-1 min-w-0">
        <p class="text-sm font-semibold text-foreground m-0 mb-1">Download speech model</p>
        {#if modelStepDone}
          <p class="text-sm text-muted-foreground m-0">Model ready</p>
        {:else}
          <p class="text-sm text-muted-foreground m-0 mb-3 leading-snug">
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
            <div class="flex gap-2.5 items-center mt-3">
              <Button onclick={downloadRecommendedModel}>Download Recommended Model</Button>
              <Button variant="ghost" onclick={() => onNavigate('models')}>
                Choose a different model
              </Button>
            </div>
          {:else if setupState === 'error'}
            <div class="flex gap-2.5 items-center mt-3">
              <Button onclick={retryDownload}>Retry Download</Button>
            </div>
          {/if}
        {/if}
      </div>
    </div>

    <!-- Step 2: Allow microphone -->
    <div
      class={[
        'flex gap-3.5 p-4 bg-secondary border rounded-md transition-colors',
        micStepDone && 'border-green-500/30',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <div
        class={[
          'w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0 text-sm font-semibold',
          !micStepDone && 'bg-muted text-muted-foreground',
          micStepDone && 'bg-green-500/15 text-green-600',
        ]
          .filter(Boolean)
          .join(' ')}
      >
        {#if micStepDone}
          <svg
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="w-3.5 h-3.5"
          >
            <path d="M3 8.5l3.5 3.5 6.5-7" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        {:else}
          2
        {/if}
      </div>
      <div class="flex-1 min-w-0">
        <p class="text-sm font-semibold text-foreground m-0 mb-1">Allow microphone access</p>
        {#if micStepDone}
          <p class="text-sm text-muted-foreground m-0">Microphone access granted</p>
        {:else}
          <p class="text-sm text-muted-foreground m-0 mb-3 leading-snug">
            Thoth needs to hear you to transcribe your speech.
          </p>
          <div class="flex gap-2.5 items-center">
            <Button onclick={() => requestPermission('request_microphone_permission')}>Allow</Button
            >
          </div>
        {/if}
      </div>
    </div>

    <!-- Step 3: Allow global shortcut -->
    <div
      class={[
        'flex gap-3.5 p-4 bg-secondary border rounded-md transition-colors',
        shortcutStepDone && 'border-green-500/30',
        accessibilityStale && 'border-yellow-500/40',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <div
        class={[
          'w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0 text-sm font-semibold',
          !shortcutStepDone && !accessibilityStale && 'bg-muted text-muted-foreground',
          shortcutStepDone && 'bg-green-500/15 text-green-600',
          accessibilityStale && 'bg-yellow-500/15 text-yellow-600 font-bold',
        ]
          .filter(Boolean)
          .join(' ')}
      >
        {#if shortcutStepDone}
          <svg
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="w-3.5 h-3.5"
          >
            <path d="M3 8.5l3.5 3.5 6.5-7" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        {:else if accessibilityStale}
          !
        {:else}
          3
        {/if}
      </div>
      <div class="flex-1 min-w-0">
        <p class="text-sm font-semibold text-foreground m-0 mb-1">Allow global shortcut</p>
        {#if shortcutStepDone}
          <p class="text-sm text-muted-foreground m-0">Shortcut access granted</p>
        {:else if accessibilityStale}
          <p class="text-sm text-yellow-600 m-0 mb-3 leading-snug">
            Permission appears granted but isn't working. This can happen after app updates or
            reinstalls.
          </p>
          <div class="flex gap-2.5 items-center">
            <Button
              variant="destructive"
              onclick={() => resetPermissions(['Accessibility', 'ListenEvent'])}
              disabled={resettingPermissions}
            >
              {resettingPermissions ? 'Resetting...' : 'Reset & Re-grant'}
            </Button>
          </div>
          {#if resetError}
            <p class="mt-1.5 text-xs text-destructive">{resetError}</p>
          {/if}
        {:else}
          <p class="text-sm text-muted-foreground m-0 mb-3 leading-snug">
            Lets you start recording from anywhere with a keyboard shortcut.
          </p>
          <div class="flex gap-2.5 items-center">
            <Button onclick={() => requestPermission('request_accessibility')}>Allow</Button>
          </div>
        {/if}
      </div>
    </div>
  </div>

  <!-- Optional settings -->
  <details class="mt-2">
    <summary class="optional-summary">Optional settings</summary>
    <div class="mt-2 p-3.5 bg-secondary border rounded-md flex flex-col gap-0.5">
      <div class="flex items-center gap-2.5 py-1.5">
        <span
          class="w-2 h-2 rounded-full flex-shrink-0"
          class:bg-green-500={inputMonitoringPermission === 'granted'}
          class:bg-yellow-500={inputMonitoringPermission === 'denied'}
          class:bg-muted-foreground={inputMonitoringPermission === 'unknown'}
        ></span>
        <span class="text-sm text-muted-foreground w-[110px] flex-shrink-0">Input Monitoring</span>
        <span class="text-sm text-foreground font-medium">
          {#if inputMonitoringPermission === 'granted'}
            Granted
          {:else}
            <Button
              size="sm"
              variant="outline"
              onclick={() => requestPermission('request_input_monitoring')}
            >
              Grant Access
            </Button>
          {/if}
        </span>
      </div>
      {#if inputMonitoringPermission !== 'granted'}
        <p class="text-xs text-muted-foreground ml-[18px] -mt-0.5 mb-1 leading-snug">
          Needed only if you want to customise the recording shortcut
        </p>
      {/if}
      <div class="flex items-center gap-2.5 py-1.5">
        <span
          class={[
            'w-2 h-2 rounded-full flex-shrink-0',
            ollamaStatus === 'connected' && 'bg-green-500',
            ollamaStatus === 'not-configured' && 'bg-muted-foreground',
            ollamaStatus === 'unavailable' && 'bg-yellow-500',
            ollamaStatus === 'checking' && 'animate-pulse bg-muted-foreground/50',
          ]
            .filter(Boolean)
            .join(' ')}
        ></span>
        <span class="text-sm text-muted-foreground w-[110px] flex-shrink-0">AI Enhancement</span>
        <span class="text-sm text-foreground font-medium">
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
      <div class="flex items-center justify-between py-1.5">
        <span class="text-sm text-muted-foreground">Launch at Login</span>
        <Switch
          checked={autostartEnabled}
          disabled={autostartLoading}
          onCheckedChange={handleAutostartToggle}
        />
      </div>
      {#if autostartError}
        <Alert.Root variant="destructive" class="mt-1">
          <Alert.Description>{autostartError}</Alert.Description>
        </Alert.Root>
      {/if}
      <div class="flex items-center justify-between py-1.5">
        <span class="text-sm text-muted-foreground">Show in Dock</span>
        <Switch checked={showInDock} disabled={dockLoading} onCheckedChange={handleDockToggle} />
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
      <div class="grid grid-cols-2 gap-2.5">
        <Card.Root>
          <Card.Content class="p-4">
            <span
              class="text-[22px] font-semibold text-foreground tabular-nums leading-tight block"
            >
              {stats.totalCount}
            </span>
            <span class="text-xs text-muted-foreground mt-1 block">Transcriptions</span>
          </Card.Content>
        </Card.Root>
        <Card.Root>
          <Card.Content class="p-4">
            <span
              class="text-[22px] font-semibold text-foreground tabular-nums leading-tight block"
            >
              {formatTotalDuration(stats.totalAudioDuration)}
            </span>
            <span class="text-xs text-muted-foreground mt-1 block">Total audio</span>
          </Card.Content>
        </Card.Root>
        <Card.Root>
          <Card.Content class="p-4">
            <span
              class="text-[22px] font-semibold text-foreground tabular-nums leading-tight block"
            >
              {stats.enhancedCount}
            </span>
            <span class="text-xs text-muted-foreground mt-1 block">Enhanced</span>
          </Card.Content>
        </Card.Root>
        <Card.Root>
          <Card.Content class="p-4">
            <span
              class="text-[22px] font-semibold text-foreground tabular-nums leading-tight block"
            >
              {avgRecordingDuration > 0 ? formatDuration(avgRecordingDuration) : '--'}
            </span>
            <span class="text-xs text-muted-foreground mt-1 block">Avg recording</span>
          </Card.Content>
        </Card.Root>
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
              <Badge variant="default">Update available: {updaterState.updateVersion}</Badge>
            {:else if updaterState.state === 'up-to-date'}
              Up to date
            {:else if updaterState.state === 'error'}
              <Badge variant="destructive">{updaterState.error || 'Check failed'}</Badge>
            {:else}
              Not checked
            {/if}
          </span>
        </div>
        <div class="autostart-row">
          <span class="status-label">Check on Launch</span>
          <Switch
            bind:checked={configStore.general.checkForUpdates}
            onCheckedChange={async () => {
              await configStore.save();
            }}
          />
        </div>
        <div class="autostart-row">
          <span class="status-label">
            {#if updaterState.state === 'checking'}
              Checking...
            {:else}
              Check Now
            {/if}
          </span>
          <Button
            size="sm"
            variant="outline"
            onclick={() => checkForUpdate()}
            disabled={updaterState.state === 'checking'}
          >
            {updaterState.state === 'checking' ? 'Checking...' : 'Check for Updates'}
          </Button>
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
          <span
            class="status-dot"
            class:ready={allPermissionsGranted}
            class:warning={!allPermissionsGranted && accessibilityPermission !== 'stale'}
            class:stale={accessibilityPermission === 'stale'}
          ></span>
          <span class="status-label">Permissions</span>
          <span class="status-value">
            {#if allPermissionsGranted}
              All granted
            {:else if accessibilityPermission === 'stale'}
              Accessibility stale
            {:else}
              {[
                microphonePermission !== 'granted' ? 'Mic' : '',
                accessibilityPermission !== 'granted' ? 'Accessibility' : '',
                inputMonitoringPermission !== 'granted' ? 'Input Monitoring' : '',
              ]
                .filter(Boolean)
                .join(', ')} needed
            {/if}
          </span>
        </div>
        {#if accessibilityPermission === 'stale'}
          <div class="stale-recovery">
            <p class="stale-recovery-desc">
              Accessibility appears granted in System Settings but isn't working — common after an
              app update or reinstall. Reset clears the stale entry so macOS will prompt again.
            </p>
            <div class="stale-recovery-actions">
              <Button
                variant="destructive"
                size="sm"
                onclick={handleResetAccessibility}
                disabled={resettingAccessibility}
              >
                {resettingAccessibility ? 'Resetting...' : 'Reset Accessibility Permission'}
              </Button>
              <Button
                variant="outline"
                size="sm"
                onclick={() => invoke('open_privacy_pane', { pane: 'accessibility' })}
              >
                Open System Settings
              </Button>
            </div>
            {#if resetAccessibilityMessage}
              <p class="stale-recovery-ok">
                {resetAccessibilityMessage} — re-enable Thoth in System Settings &rarr; Privacy &amp;
                Security &rarr; Accessibility.
              </p>
            {/if}
            {#if resetAccessibilityError}
              <p class="mt-1.5 text-xs text-destructive">{resetAccessibilityError}</p>
            {/if}
          </div>
        {/if}
        <div class="autostart-row">
          <span class="status-label">Launch at Login</span>
          <Switch
            checked={autostartEnabled}
            disabled={autostartLoading}
            onCheckedChange={handleAutostartToggle}
          />
        </div>
        {#if autostartError}
          <Alert.Root variant="destructive" class="my-1">
            <Alert.Description>{autostartError}</Alert.Description>
          </Alert.Root>
        {/if}
        <div class="autostart-row">
          <span class="status-label">Show in Dock</span>
          <Switch checked={showInDock} disabled={dockLoading} onCheckedChange={handleDockToggle} />
        </div>
      </div>
    </div>
  </section>

  <!-- Troubleshooting (advanced) -->
  <details class="mt-2">
    <summary class="optional-summary">Troubleshooting</summary>
    <div class="mt-2 p-3.5 bg-secondary border rounded-md flex flex-col gap-2">
      <p class="text-sm text-muted-foreground leading-snug m-0">
        After installing a new version of Thoth, macOS may block it or hold onto stale permission
        entries. Run these steps in order — each one takes about 2 seconds.
      </p>

      <!-- Step 1: Quarantine -->
      <div class="bg-card border rounded-md p-2.5 mb-0">
        <div class="flex items-center gap-2.5">
          <span
            class="w-[22px] h-[22px] rounded-full bg-primary text-primary-foreground text-[11px] font-bold flex items-center justify-center flex-shrink-0"
            >1</span
          >
          <div class="flex-1 flex flex-col gap-0.5">
            <span class="text-sm font-medium text-foreground">Remove quarantine flag</span>
            <span class="text-xs text-muted-foreground leading-snug">
              macOS marks downloaded apps as quarantined. Removes the "are you sure?" block.
            </span>
          </div>
          <Button
            size="sm"
            variant="outline"
            onclick={async () => {
              try {
                await invoke('remove_quarantine');
                permFixStatus.quarantine = 'done';
              } catch (e) {
                permFixStatus.quarantine = 'error';
              }
            }}
          >
            {permFixStatus.quarantine === 'done'
              ? '✓ Done'
              : permFixStatus.quarantine === 'error'
                ? '✗ Error'
                : 'Fix'}
          </Button>
        </div>
      </div>

      <!-- Step 2: Reset all TCC -->
      <AlertDialog.Root>
        <div class="bg-card border rounded-md p-2.5 mb-0">
          <div class="flex items-center gap-2.5">
            <span
              class="w-[22px] h-[22px] rounded-full bg-primary text-primary-foreground text-[11px] font-bold flex items-center justify-center flex-shrink-0"
              >2</span
            >
            <div class="flex-1 flex flex-col gap-0.5">
              <span class="text-sm font-medium text-foreground">Reset system permissions</span>
              <span class="text-xs text-muted-foreground leading-snug">
                Clears stale grants for Input Monitoring, Accessibility, and Microphone. macOS will
                re-prompt for each one.
              </span>
            </div>
            <AlertDialog.Trigger>
              {#snippet child({ props })}
                <Button size="sm" variant="destructive" {...props}>
                  {permFixStatus.tcc === 'done'
                    ? '✓ Done'
                    : permFixStatus.tcc === 'error'
                      ? '✗ Error'
                      : 'Reset'}
                </Button>
              {/snippet}
            </AlertDialog.Trigger>
          </div>
        </div>
        <AlertDialog.Portal>
          <AlertDialog.Overlay />
          <AlertDialog.Content>
            <AlertDialog.Header>
              <AlertDialog.Title>Reset System Permissions?</AlertDialog.Title>
              <AlertDialog.Description>
                This will clear Input Monitoring, Accessibility, and Microphone grants. macOS will
                re-prompt for each permission. You will need to re-grant access before Thoth can
                record again.
              </AlertDialog.Description>
            </AlertDialog.Header>
            <AlertDialog.Footer>
              <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
              <AlertDialog.Action
                onclick={async () => {
                  try {
                    await invoke('reset_tcc_permissions', {
                      services: ['ListenEvent', 'Accessibility', 'Microphone'],
                    });
                    permFixStatus.tcc = 'done';
                  } catch (e) {
                    permFixStatus.tcc = 'error';
                  }
                }}
              >
                Reset Permissions
              </AlertDialog.Action>
            </AlertDialog.Footer>
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>

      <!-- Step 3: Open privacy panes -->
      <div class="bg-card border rounded-md p-2.5 mb-0">
        <div class="flex items-center gap-2.5">
          <span
            class="w-[22px] h-[22px] rounded-full bg-primary text-primary-foreground text-[11px] font-bold flex items-center justify-center flex-shrink-0"
            >3</span
          >
          <div class="flex-1 flex flex-col gap-0.5">
            <span class="text-sm font-medium text-foreground">Open Privacy &amp; Security</span>
            <span class="text-xs text-muted-foreground leading-snug">
              Opens the two panels where you'll re-grant permissions after relaunching Thoth.
            </span>
          </div>
          <div class="flex gap-1">
            <Button
              size="sm"
              variant="outline"
              onclick={() => invoke('open_privacy_pane', { pane: 'accessibility' })}
            >
              Accessibility
            </Button>
            <Button
              size="sm"
              variant="outline"
              onclick={() => invoke('open_privacy_pane', { pane: 'input-monitoring' })}
            >
              Input Monitoring
            </Button>
          </div>
        </div>
      </div>

      <!-- Step 4: Quit and relaunch -->
      <div class="bg-card border rounded-md p-2.5 mb-0">
        <div class="flex items-center gap-2.5">
          <span
            class="w-[22px] h-[22px] rounded-full bg-primary text-primary-foreground text-[11px] font-bold flex items-center justify-center flex-shrink-0"
            >4</span
          >
          <div class="flex-1 flex flex-col gap-0.5">
            <span class="text-sm font-medium text-foreground">Quit &amp; relaunch Thoth</span>
            <span class="text-xs text-muted-foreground leading-snug">
              Restart the app so macOS re-prompts for each permission. Click Allow in each dialog.
            </span>
          </div>
          <Button
            size="sm"
            variant="outline"
            onclick={async () => {
              await invoke('relaunch_app').catch(() => {});
            }}
          >
            Relaunch
          </Button>
        </div>
      </div>

      {#if resetError}
        <p class="mt-1.5 text-xs text-destructive">{resetError}</p>
      {/if}

      <details class="mt-2">
        <summary class="text-xs text-muted-foreground cursor-pointer">
          Manual fix (Terminal)
        </summary>
        <div class="mt-2 p-2.5 bg-muted rounded-sm">
          <code class="manual-fix-code">xattr -dr com.apple.quarantine /Applications/Thoth.app</code
          >
          <code class="manual-fix-code">tccutil reset ListenEvent com.poodle64.thoth</code>
          <code class="manual-fix-code">tccutil reset Accessibility com.poodle64.thoth</code>
          <code class="manual-fix-code">tccutil reset Microphone com.poodle64.thoth</code>
          <p class="mt-1.5 text-xs text-muted-foreground">
            Then restart Thoth and re-grant permissions in System Settings &rarr; Privacy &amp;
            Security.
          </p>
        </div>
      </details>
    </div>
  </details>
{/if}

<style>
  /* Celebration animation */
  .celebrating :global(.empty-title) {
    animation: celebrateText 0.6s ease-out;
  }

  @keyframes celebrateText {
    0% {
      transform: scale(1);
    }
    50% {
      transform: scale(1.05);
    }
    100% {
      transform: scale(1);
    }
  }

  /* Optional settings summary toggle */
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

  /* Progress bar (kept as bespoke — not a shadcn primitive) */
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
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(350%);
    }
  }

  /* System status list */
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

  .status-dot.stale {
    background: var(--color-warning);
  }

  .status-dot.checking {
    background: var(--color-text-tertiary);
    animation: pulse 1s ease-in-out infinite;
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

  .autostart-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 0;
  }

  /* Truncation */
  .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  /* Stale accessibility recovery block */
  .stale-recovery {
    padding: 10px 12px;
    margin: 2px 0 4px;
    background: color-mix(in srgb, var(--color-warning) 8%, var(--color-bg-secondary));
    border: 1px solid color-mix(in srgb, var(--color-warning) 30%, var(--color-border-subtle));
    border-radius: var(--radius-md);
  }

  .stale-recovery-desc {
    margin: 0 0 10px;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.4;
  }

  .stale-recovery-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .stale-recovery-ok {
    margin: 8px 0 0;
    font-size: var(--text-xs);
    color: var(--color-success);
    line-height: 1.4;
  }

  /* Manual fix terminal code */
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
</style>
