<script lang="ts">
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { invoke } from '@tauri-apps/api/core';
  import * as Dialog from '$components/ui/dialog';
  import { Button } from '$components/ui/button';

  interface Props {
    open: boolean;
    onclose: () => void;
  }

  let { open, onclose }: Props = $props();

  let version = $state('');

  onMount(async () => {
    try {
      version = await getVersion();
    } catch {
      version = '';
    }
  });

  function openExternal(url: string) {
    invoke('open_url', { url }).catch((err) => console.error('Failed to open URL:', err));
  }
</script>

<Dialog.Root
  {open}
  onOpenChange={(v) => {
    if (!v) onclose();
  }}
>
  <Dialog.Content class="max-w-sm text-center" showCloseButton={false}>
    <Dialog.Header class="items-center">
      <span class="text-7xl leading-none mb-2">𓅝</span>
      <Dialog.Title class="text-xl font-bold tracking-wide">Thoth</Dialog.Title>
      <Dialog.Description class="italic">Scribe to the gods. Typist to you.</Dialog.Description>
      {#if version}
        <p class="text-xs text-muted-foreground tabular-nums">Version {version}</p>
      {/if}
    </Dialog.Header>

    <div class="flex flex-col gap-1 text-sm text-muted-foreground">
      <p>
        Created by
        <Button
          variant="link"
          class="h-auto p-0 text-sm"
          onclick={() => openExternal('https://github.com/poodle64')}
        >
          poodle64
        </Button>
      </p>
      <p>
        Contributions from
        <Button
          variant="link"
          class="h-auto p-0 text-sm"
          onclick={() => openExternal('https://github.com/nephalemsec')}
        >
          nephalemsec
        </Button>
      </p>
    </div>

    <div class="flex items-center justify-center gap-2 text-xs">
      <Button
        variant="link"
        class="h-auto p-0 text-xs"
        onclick={() => openExternal('https://github.com/poodle64/thoth')}
      >
        GitHub
      </Button>
      <span class="text-muted-foreground">·</span>
      <Button
        variant="link"
        class="h-auto p-0 text-xs"
        onclick={() => openExternal('https://github.com/poodle64/thoth/blob/main/LICENCE')}
      >
        MIT Licence
      </Button>
    </div>

    <div class="border-t pt-4 flex flex-col gap-1">
      <p class="text-xs text-muted-foreground uppercase tracking-wide">Built with</p>
      <p class="text-xs text-muted-foreground leading-relaxed">
        Tauri · Svelte · whisper.cpp · Sherpa-ONNX · Ollama
      </p>
    </div>

    <Dialog.Footer class="justify-center sm:justify-center">
      <Dialog.Close>
        {#snippet child({ props })}
          <Button variant="secondary" {...props} onclick={onclose}>Close</Button>
        {/snippet}
      </Dialog.Close>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
