<script lang="ts">
	import { getUpdaterState, downloadAndInstall, dismissUpdate } from '$lib/stores/updater.svelte';

	const updaterState = getUpdaterState();
</script>

{#if updaterState.state === 'available' || updaterState.state === 'downloading' || updaterState.state === 'ready'}
	<div class="update-banner">
		<div class="update-content">
			<div class="update-icon">
				<svg width="20" height="20" viewBox="0 0 20 20" fill="none" xmlns="http://www.w3.org/2000/svg">
					<path
						d="M10 2L10 12M10 12L7 9M10 12L13 9"
						stroke="currentColor"
						stroke-width="2"
						stroke-linecap="round"
						stroke-linejoin="round"
					/>
					<path
						d="M4 16L4 17C4 17.5304 4.21071 18.0391 4.58579 18.4142C4.96086 18.7893 5.46957 19 6 19L14 19C14.5304 19 15.0391 18.7893 15.4142 18.4142C15.7893 18.0391 16 17.5304 16 17V16"
						stroke="currentColor"
						stroke-width="2"
						stroke-linecap="round"
					/>
				</svg>
			</div>
			<div class="update-text">
				{#if updaterState.state === 'available'}
					<div class="update-title">Update Available</div>
					<div class="update-version">
						Version {updaterState.updateVersion} is ready to install
					</div>
				{:else if updaterState.state === 'downloading'}
					<div class="update-title">Downloading Update...</div>
					<div class="update-progress-bar">
						<div class="progress-fill" style="width: {updaterState.downloadProgress}%"></div>
					</div>
				{:else if updaterState.state === 'ready'}
					<div class="update-title">Update Ready</div>
					<div class="update-version">Click Restart to apply the update</div>
				{/if}
			</div>
		</div>
		<div class="update-actions">
			{#if updaterState.state === 'available'}
				<button class="btn-update" onclick={() => downloadAndInstall()}>Update Now</button>
				<button class="btn-later" onclick={() => dismissUpdate()}>Later</button>
			{:else if updaterState.state === 'ready'}
				<button class="btn-update" onclick={() => downloadAndInstall()}>Restart</button>
				<button class="btn-later" onclick={() => dismissUpdate()}>Later</button>
			{:else}
				<!-- Downloading - show progress only -->
				<div class="download-progress">{updaterState.downloadProgress}%</div>
			{/if}
		</div>
	</div>
{/if}

<style>
	.update-banner {
		display: flex;
		align-items: centre;
		justify-content: space-between;
		padding: var(--spacing-3);
		background: color-mix(in srgb, var(--color-accent) 10%, transparent);
		border: 1px solid color-mix(in srgb, var(--color-accent) 30%, transparent);
		border-radius: var(--radius-md);
		margin-bottom: var(--spacing-4);
	}

	.update-content {
		display: flex;
		align-items: centre;
		gap: var(--spacing-3);
	}

	.update-icon {
		color: var(--color-accent);
		flex-shrink: 0;
	}

	.update-text {
		display: flex;
		flex-direction: column;
		gap: var(--spacing-1);
	}

	.update-title {
		font-weight: 600;
		font-size: var(--text-sm);
		color: var(--color-text);
	}

	.update-version {
		font-size: var(--text-xs);
		color: var(--color-text-muted);
	}

	.update-progress-bar {
		width: 200px;
		height: 4px;
		background: color-mix(in srgb, var(--color-accent) 20%, transparent);
		border-radius: var(--radius-full);
		overflow: hidden;
	}

	.progress-fill {
		height: 100%;
		background: var(--color-accent);
		transition: width 0.3s ease;
	}

	.update-actions {
		display: flex;
		align-items: centre;
		gap: var(--spacing-2);
	}

	.btn-update {
		padding: var(--spacing-1-5) var(--spacing-3);
		background: var(--color-accent);
		color: white;
		border: none;
		border-radius: var(--radius-sm);
		font-size: var(--text-sm);
		font-weight: 500;
		cursor: pointer;
		transition: all 0.2s ease;
	}

	.btn-update:hover {
		background: color-mix(in srgb, var(--color-accent) 90%, black);
	}

	.btn-later {
		padding: var(--spacing-1-5) var(--spacing-3);
		background: transparent;
		color: var(--color-text-muted);
		border: none;
		border-radius: var(--radius-sm);
		font-size: var(--text-sm);
		cursor: pointer;
		transition: all 0.2s ease;
	}

	.btn-later:hover {
		background: color-mix(in srgb, var(--color-text) 5%, transparent);
		color: var(--color-text);
	}

	.download-progress {
		font-size: var(--text-sm);
		color: var(--color-accent);
		font-weight: 600;
	}
</style>
