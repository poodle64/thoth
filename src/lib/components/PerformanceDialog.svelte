<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { formatDuration, formatTotalDuration, formatSpeedFactor } from '../utils/format';
  import * as Dialog from '$components/ui/dialog';
  import * as Card from '$components/ui/card';
  import * as Alert from '$components/ui/alert';
  import { Skeleton } from '$components/ui/skeleton';
  import AlertCircleIcon from '@lucide/svelte/icons/alert-circle';

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
    /** Whether the dialog is visible */
    open: boolean;
  }

  let { open = $bindable() }: Props = $props();

  let stats = $state<TranscriptionStats | null>(null);
  let isLoading = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    if (open) {
      loadStats();
    }
  });

  async function loadStats() {
    isLoading = true;
    error = null;
    try {
      stats = await invoke<TranscriptionStats>('get_transcription_stats_cmd');
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isLoading = false;
    }
  }

  function close() {
    open = false;
  }
</script>

<Dialog.Root
  bind:open
  onOpenChange={(v) => {
    if (!v) close();
  }}
>
  <Dialog.Content
    class="max-w-[560px] max-h-[80vh] overflow-hidden flex flex-col"
    showCloseButton={false}
  >
    <Dialog.Header>
      <Dialog.Title>Performance Analysis</Dialog.Title>
    </Dialog.Header>

    <div class="overflow-y-auto flex-1 py-2 flex flex-col gap-4">
      {#if isLoading}
        <div class="flex flex-col gap-3">
          <div class="grid grid-cols-2 gap-2">
            <Skeleton class="h-16 rounded-xl" />
            <Skeleton class="h-16 rounded-xl" />
            <Skeleton class="h-16 rounded-xl" />
            <Skeleton class="h-16 rounded-xl" />
          </div>
          <Skeleton class="h-24 rounded-xl" />
        </div>
      {:else if error}
        <Alert.Root variant="destructive">
          <AlertCircleIcon />
          <Alert.Description>{error}</Alert.Description>
        </Alert.Root>
      {:else if stats}
        <!-- Summary cards -->
        <div class="grid grid-cols-2 gap-2">
          {#each [{ value: stats.totalCount, label: 'Total transcriptions' }, { value: stats.analysableCount, label: 'With performance data' }, { value: stats.enhancedCount, label: 'Enhanced' }, { value: formatTotalDuration(stats.totalAudioDuration), label: 'Total audio' }] as item}
            <Card.Root size="sm">
              <Card.Content class="flex flex-col items-center py-3 px-3">
                <span class="text-xl font-semibold tabular-nums">{item.value}</span>
                <span class="text-xs text-muted-foreground mt-1">{item.label}</span>
              </Card.Content>
            </Card.Root>
          {/each}
        </div>

        <!-- Transcription model performance -->
        {#if stats.transcriptionModels.length > 0}
          <section class="flex flex-col gap-2">
            <h3 class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">
              Transcription models
            </h3>
            <div class="flex flex-col gap-2">
              {#each stats.transcriptionModels as model}
                <Card.Root size="sm">
                  <Card.Content class="py-2">
                    <p class="text-sm font-medium font-mono mb-1">{model.name}</p>
                    <div class="flex gap-4 flex-wrap">
                      <span class="flex items-baseline gap-1">
                        <span class="text-sm font-semibold text-primary tabular-nums"
                          >{model.count}</span
                        >
                        <span class="text-xs text-muted-foreground">uses</span>
                      </span>
                      <span class="flex items-baseline gap-1">
                        <span class="text-sm font-semibold text-primary tabular-nums"
                          >{formatSpeedFactor(model.speedFactor)}</span
                        >
                        <span class="text-xs text-muted-foreground">RTFX</span>
                      </span>
                      <span class="flex items-baseline gap-1">
                        <span class="text-sm font-semibold text-primary tabular-nums"
                          >{formatDuration(model.avgAudioDuration)}</span
                        >
                        <span class="text-xs text-muted-foreground">avg audio</span>
                      </span>
                      <span class="flex items-baseline gap-1">
                        <span class="text-sm font-semibold text-primary tabular-nums"
                          >{formatDuration(model.avgProcessingTime)}</span
                        >
                        <span class="text-xs text-muted-foreground">avg time</span>
                      </span>
                    </div>
                  </Card.Content>
                </Card.Root>
              {/each}
            </div>
          </section>
        {/if}

        <!-- Enhancement model performance -->
        {#if stats.enhancementModels.length > 0}
          <section class="flex flex-col gap-2">
            <h3 class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">
              Enhancement models
            </h3>
            <div class="flex flex-col gap-2">
              {#each stats.enhancementModels as model}
                <Card.Root size="sm">
                  <Card.Content class="py-2">
                    <p class="text-sm font-medium font-mono mb-1">{model.name}</p>
                    <div class="flex gap-4 flex-wrap">
                      <span class="flex items-baseline gap-1">
                        <span class="text-sm font-semibold text-primary tabular-nums"
                          >{model.count}</span
                        >
                        <span class="text-xs text-muted-foreground">uses</span>
                      </span>
                      <span class="flex items-baseline gap-1">
                        <span class="text-sm font-semibold text-primary tabular-nums"
                          >{formatDuration(model.avgProcessingTime)}</span
                        >
                        <span class="text-xs text-muted-foreground">avg time</span>
                      </span>
                    </div>
                  </Card.Content>
                </Card.Root>
              {/each}
            </div>
          </section>
        {/if}

        {#if stats.transcriptionModels.length === 0 && stats.enhancementModels.length === 0}
          <div class="text-center py-8 text-sm text-muted-foreground">
            <p>No performance data recorded yet.</p>
            <p class="mt-2 text-xs opacity-70">
              Statistics will appear after transcriptions are created with the updated app.
            </p>
          </div>
        {/if}
      {/if}
    </div>

    <Dialog.Footer showCloseButton={true} />
  </Dialog.Content>
</Dialog.Root>
