<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { configStore } from '../stores/config.svelte';
  import { toast } from 'svelte-sonner';
  import { Switch } from '$components/ui/switch';
  import { Button } from '$components/ui/button';
  import { Input } from '$components/ui/input';
  import { Label } from '$components/ui/label';
  import { Badge } from '$components/ui/badge';
  import * as Tooltip from '$components/ui/tooltip';
  import Info from '@lucide/svelte/icons/info';

  // The API mask sentinel — must match LOKI_AUTH_MASK in config.rs.
  const LOKI_AUTH_MASK = '***';

  let isTesting = $state(false);
  let isSaving = $state(false);
  // Tracks whether the token field has been edited in this session.
  // When dirty, we save via set_loki_auth on blur; otherwise skip to avoid
  // unintentionally clearing the stored token with the mask value.
  let lokiAuthDirty = $state(false);

  async function saveSettings(): Promise<void> {
    await configStore.save();
  }

  async function handleSave(): Promise<void> {
    isSaving = true;
    try {
      // Persist the token via the dedicated command (bypasses the preservation
      // guard) when edited, then save the rest of the config.
      if (lokiAuthDirty) {
        await invoke('set_loki_auth', { token: configStore.logging.lokiAuth || null });
        lokiAuthDirty = false;
      }
      await saveSettings();
      toast.success('Logging settings saved', {
        description: configStore.logging.remoteEnabled
          ? 'Remote forwarding applies after the next app restart.'
          : undefined,
      });
    } catch (e) {
      toast.error('Failed to save logging settings', {
        description: e instanceof Error ? e.message : String(e),
      });
    } finally {
      isSaving = false;
    }
  }

  async function handleRetentionInput(event: Event): Promise<void> {
    const input = event.target as HTMLInputElement;
    const days = Math.max(1, Math.min(365, parseInt(input.value, 10) || 7));
    configStore.updateLogging('localRetentionDays', days);
  }

  async function handleRetentionBlur(): Promise<void> {
    await saveSettings();
  }

  async function handleRemoteToggle(enabled: boolean): Promise<void> {
    configStore.updateLogging('remoteEnabled', enabled);
    await saveSettings();
  }

  function handleLokiUrlInput(event: Event): void {
    const input = event.target as HTMLInputElement;
    configStore.updateLogging('lokiUrl', input.value);
  }

  async function handleLokiUrlBlur(): Promise<void> {
    await saveSettings();
  }

  function handleLokiAuthInput(event: Event): void {
    const input = event.target as HTMLInputElement;
    lokiAuthDirty = true;
    configStore.updateLogging('lokiAuth', input.value);
  }

  async function handleLokiAuthBlur(): Promise<void> {
    if (!lokiAuthDirty) return;
    lokiAuthDirty = false;
    const token = configStore.logging.lokiAuth;
    // Use the dedicated command so the preservation guard in set_config doesn't
    // block intentional token changes (including clearing).
    try {
      await invoke('set_loki_auth', { token: token || null });
    } catch (e) {
      toast.error('Failed to save Loki token', {
        description: e instanceof Error ? e.message : String(e),
      });
    }
  }

  function handleLokiTenantInput(event: Event): void {
    const input = event.target as HTMLInputElement;
    configStore.updateLogging('lokiTenant', input.value || null);
  }

  async function handleLokiTenantBlur(): Promise<void> {
    await saveSettings();
  }

  async function handleTestConnection(): Promise<void> {
    isTesting = true;
    try {
      // Test the on-screen values directly so the user can verify before saving.
      // If the token field shows the mask sentinel, pass empty string so Loki
      // attempts an unauthenticated push (the real token stays server-side).
      const authToTest =
        configStore.logging.lokiAuth === LOKI_AUTH_MASK ? '' : configStore.logging.lokiAuth;
      await invoke('test_loki_connection', {
        url: configStore.logging.lokiUrl,
        auth: authToTest,
        tenant: configStore.logging.lokiTenant,
      });
      toast.success('Loki connection successful');
    } catch (e) {
      toast.error('Loki connection failed', {
        description: e instanceof Error ? e.message : String(e),
      });
    } finally {
      isTesting = false;
    }
  }

  onMount(async () => {
    await configStore.load();
  });
</script>

<!-- Section: Logging & Telemetry -->
<section class="flex flex-col">
  <div class="mb-3">
    <h2 class="text-base font-semibold text-foreground m-0">Logging & Telemetry</h2>
    <p class="text-xs text-muted-foreground m-0">
      Local diagnostic logs and optional structured telemetry export.
    </p>
  </div>

  <!-- One consolidated card: local logging, remote toggle, connection details,
       and the test/save actions, separated by internal dividers. -->
  <div class="divide-y divide-border rounded-md border border-border bg-card">
    <!-- Local logging (always on) -->
    <div class="flex flex-col gap-3 p-3">
      <div class="flex items-center justify-between gap-4">
        <div class="flex flex-1 flex-col gap-1">
          <span class="text-sm font-medium text-foreground">Local logging</span>
          <span class="text-xs text-muted-foreground">
            Logs are written to <code class="rounded bg-muted px-1 py-0.5 font-mono text-xs"
              >~/.thoth/logs/</code
            >
          </span>
        </div>
        <Badge variant="outline" class="border-chart-2/30 bg-chart-2/10 text-chart-2 flex-shrink-0">
          On
        </Badge>
      </div>
      <div class="flex items-center gap-3">
        <Label class="text-sm text-muted-foreground whitespace-nowrap">Keep logs for</Label>
        <Input
          type="number"
          min="1"
          max="365"
          class="w-20 text-sm"
          value={configStore.logging.localRetentionDays}
          oninput={handleRetentionInput}
          onblur={handleRetentionBlur}
          aria-label="Log retention in days"
        />
        <span class="text-sm text-muted-foreground">days</span>
      </div>
    </div>

    <!-- Forward to Loki -->
    <div class="flex items-center justify-between gap-4 p-3">
      <div class="flex flex-1 flex-col gap-1">
        <span class="text-sm font-medium text-foreground">Forward telemetry to Loki</span>
        <span class="text-xs text-muted-foreground">
          Send structured operational events to a Loki endpoint
        </span>
      </div>
      <Switch checked={configStore.logging.remoteEnabled} onCheckedChange={handleRemoteToggle} />
    </div>

    <!-- Loki connection details — shown only when remote is enabled -->
    {#if configStore.logging.remoteEnabled}
      <div class="flex flex-col gap-3 p-3">
        <!-- Loki URL -->
        <div class="flex flex-col gap-1.5">
          <Label class="text-sm font-medium">Loki URL</Label>
          <Input
            type="url"
            class="font-mono text-sm"
            value={configStore.logging.lokiUrl}
            oninput={handleLokiUrlInput}
            onblur={handleLokiUrlBlur}
            placeholder="http://loki:3100/loki/api/v1/push"
            aria-label="Loki push URL"
          />
        </div>

        <!-- Bearer token -->
        <div class="flex flex-col gap-1.5">
          <Label class="text-sm font-medium">Bearer token</Label>
          <Input
            type="password"
            class="font-mono text-sm"
            value={configStore.logging.lokiAuth}
            oninput={handleLokiAuthInput}
            onblur={handleLokiAuthBlur}
            placeholder="Paste your token"
            autocomplete="off"
            aria-label="Loki bearer token"
          />
          <p class="text-xs text-muted-foreground">
            Just the token — <span class="font-mono">Bearer</span> is added automatically. Paste a
            full
            <span class="font-mono">Bearer …</span> or <span class="font-mono">Basic …</span> value to
            override.
          </p>
        </div>

        <!-- Tenant ID (optional, with explanation) -->
        <div class="flex flex-col gap-1.5">
          <div class="flex items-center gap-1.5">
            <Label class="text-sm font-medium">Tenant ID</Label>
            <span class="text-xs font-normal text-muted-foreground">(optional)</span>
            <Tooltip.Provider delayDuration={150}>
              <Tooltip.Root>
                <Tooltip.Trigger
                  class="text-muted-foreground transition-colors hover:text-foreground"
                  aria-label="What is the Tenant ID?"
                >
                  <Info class="h-3.5 w-3.5" />
                </Tooltip.Trigger>
                <Tooltip.Content class="max-w-xs">
                  <p class="text-xs leading-snug">
                    Only for multi-tenant Loki (e.g. Grafana Cloud): sets the
                    <span class="font-mono">X-Scope-OrgID</span> header to select a tenant. Leave it blank
                    for a single-tenant instance such as a self-hosted Loki.
                  </p>
                </Tooltip.Content>
              </Tooltip.Root>
            </Tooltip.Provider>
          </div>
          <Input
            type="text"
            class="font-mono text-sm"
            value={configStore.logging.lokiTenant ?? ''}
            oninput={handleLokiTenantInput}
            onblur={handleLokiTenantBlur}
            placeholder="Leave blank unless multi-tenant"
            aria-label="Loki tenant ID"
          />
        </div>
      </div>
    {/if}

    <!-- Actions: test + save, co-located -->
    <div class="flex items-center gap-2 p-3">
      {#if configStore.logging.remoteEnabled}
        <Button
          variant="outline"
          size="sm"
          onclick={handleTestConnection}
          disabled={isTesting || !configStore.logging.lokiUrl}
        >
          {isTesting ? 'Testing…' : 'Test connection'}
        </Button>
      {/if}
      <Button size="sm" onclick={handleSave} disabled={isSaving}>
        {isSaving ? 'Saving…' : 'Save'}
      </Button>
    </div>
  </div>

  <!-- Privacy note -->
  <p class="mt-2 px-1 text-xs text-muted-foreground">
    Only content-free operational events are sent (timings, errors, model names) — never your
    transcript text. Remote forwarding takes effect after the next app restart.
  </p>
</section>
