<script lang="ts">
  import { Checkbox } from '@poodle64/ui/checkbox';
  import * as Dialog from '@poodle64/ui/dialog';

  let checked = $state(false);
</script>

<Checkbox bind:checked aria-label="smoke-test-checkbox" />

<Dialog.Root>
  <Dialog.Trigger>Open dialog</Dialog.Trigger>
  <Dialog.Content>
    <Dialog.Title>Smoke test dialog</Dialog.Title>
  </Dialog.Content>
</Dialog.Root>
