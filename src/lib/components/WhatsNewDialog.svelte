<script lang="ts">
  /**
   * Shows the running version's changelog entry, once, after an update (#113).
   *
   * The content comes from the app's own `CHANGELOG.md`, embedded at build
   * time and parsed in Rust, so there is no second set of release notes to
   * keep in step. Nothing here renders markdown — the backend has already
   * split each bullet into its bold lead and its body — because the app has
   * no HTML-injection surface anywhere and a "what's new" modal is not the
   * reason to open one.
   */
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { invoke } from '@tauri-apps/api/core';
  import { AppDialog } from '@poodle64/ui/app-dialog';
  import { DialogSection } from '@poodle64/ui/dialog-section';
  import { Button } from '@poodle64/ui/button';

  interface NotesItem {
    lead: string | null;
    body: string;
  }

  interface NotesSection {
    heading: string;
    items: NotesItem[];
  }

  interface ReleaseNotes {
    version: string;
    date: string | null;
    sections: NotesSection[];
  }

  let notes = $state<ReleaseNotes | null>(null);
  let open = $state(false);

  onMount(async () => {
    try {
      const version = await getVersion();
      // Returns null unless this version differs from the one already seen
      // AND the changelog has an entry for it — so a local build with no
      // entry shows nothing rather than an empty dialog.
      const result = await invoke<ReleaseNotes | null>('whats_new', { version });
      if (result) {
        notes = result;
        open = true;
      }
    } catch (e) {
      // Never block startup on release notes.
      console.error("Failed to load what's new:", e);
    }
  });

  async function dismiss() {
    open = false;
    if (!notes) return;
    try {
      // Marked seen only on dismissal, so a crash mid-read shows it again
      // rather than swallowing the one time it was going to appear.
      await invoke('mark_whats_new_seen', { version: notes.version });
    } catch (e) {
      console.error("Failed to record what's new as seen:", e);
    }
  }
</script>

{#if notes}
  <AppDialog
    bind:open
    onOpenChange={(v) => {
      // Fires for Escape and the close button too, not just the footer action,
      // so every route out records the version as seen.
      if (!v) dismiss();
    }}
    title="What's new in {notes.version}"
    subtitle={notes.date ?? undefined}
    size="md"
  >
    {#each notes.sections as section, i (section.heading + i)}
      <DialogSection label={section.heading || undefined}>
        <ul class="m-0 flex list-none flex-col gap-3 p-0">
          {#each section.items as item, j (j)}
            <li class="text-muted-foreground text-sm leading-relaxed">
              {#if item.lead}
                <span class="text-foreground font-medium">{item.lead}</span>
              {/if}
              {item.body}
            </li>
          {/each}
        </ul>
      </DialogSection>
    {/each}

    {#snippet footer()}
      <Button onclick={dismiss}>Got it</Button>
    {/snippet}
  </AppDialog>
{/if}
