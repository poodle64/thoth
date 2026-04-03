<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    message: string;
    action?: Snippet;
    class?: string;
  }

  let { message, action, class: className = '' }: Props = $props();
</script>

<div class="flex flex-col items-center justify-center gap-4 py-12 text-center {className}">
  <div class="rounded-full bg-error/10 p-3">
    <svg class="h-6 w-6 text-error" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <circle cx="12" cy="12" r="10" />
      <line x1="12" y1="8" x2="12" y2="12" />
      <line x1="12" y1="16" x2="12.01" y2="16" />
    </svg>
  </div>
  <p class="text-sm text-error">{message}</p>
  {#if action}
    {@render action()}
  {/if}
</div>
