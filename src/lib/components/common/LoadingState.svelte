<script lang="ts">
  import Loader2 from '@lucide/svelte/icons/loader-2';

  interface Props {
    message?: string;
    class?: string;
  }

  let { message = 'Loading...', class: className = '' }: Props = $props();
</script>

<div
  role="status"
  aria-live="polite"
  class="flex flex-col items-center justify-center gap-3 py-12 {className}"
>
  <Loader2 class="h-8 w-8 animate-spin text-primary" />
  <p class="text-sm text-text-secondary">{message}</p>
</div>
