<script lang="ts">
  import { Toaster as SonnerToaster, type ToasterProps } from 'svelte-sonner';
  import { mode } from 'mode-watcher';

  let props: ToasterProps = $props();
</script>

<SonnerToaster theme={mode.current} {...props} />
