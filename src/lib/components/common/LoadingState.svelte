<script lang="ts">
  interface Props {
    message?: string;
    class?: string;
  }

  let { message = 'Loading...', class: className = '' }: Props = $props();
</script>

<div class="flex flex-col items-center justify-center gap-3 py-12 {className}">
  <div class="h-8 w-8 animate-spin rounded-full border-2 border-accent border-t-transparent"></div>
  <p class="text-sm text-text-secondary">{message}</p>
</div>
