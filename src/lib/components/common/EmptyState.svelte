<script lang="ts">
  import type { Snippet, Component } from 'svelte';

  interface Props {
    icon?: Component;
    title: string;
    description?: string;
    action?: Snippet;
    class?: string;
  }

  let { icon: Icon, title, description, action, class: className = '' }: Props = $props();
</script>

<div class="flex flex-col items-center justify-center gap-4 py-12 text-center {className}">
  {#if Icon}
    <Icon class="h-12 w-12 text-text-tertiary" />
  {/if}
  <div class="space-y-1">
    <h3 class="text-lg font-medium text-text-primary">{title}</h3>
    {#if description}
      <p class="text-sm text-text-secondary">{description}</p>
    {/if}
  </div>
  {#if action}
    {@render action()}
  {/if}
</div>
