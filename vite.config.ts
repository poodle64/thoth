import { defineConfig } from 'vite';
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import type { Plugin } from 'vite';

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

/**
 * Keep Tailwind out of Svelte's scoped component styles.
 *
 * A Svelte `<style>` block compiles to a virtual module whose id ends in
 * `&lang.css` (e.g. `Settings.svelte?svelte&type=style&lang.css`). The
 * `@tailwindcss/vite` transform filter matches `&lang.css`, so it treats every
 * component style as a Tailwind entrypoint. Under Vite 8 + vite-plugin-svelte 7
 * the dev server intermittently hands the plugin the raw `.svelte` source (the
 * `<script>` block) before Svelte has extracted the CSS, so Tailwind's CSS
 * parser blows up on the first JS token — the `Invalid declaration: \`invoke\``
 * 500s seen in the dev log.
 *
 * No component style in this project uses Tailwind directives
 * (@apply/@reference/@tailwind/@variant) — the sole Tailwind entry is
 * `src/app.css` — so excluding component-style virtual modules from Tailwind's
 * transform is a no-op for output: Svelte still serves the scoped CSS untouched.
 */
function tailwindSkippingSvelteStyles(): Plugin[] {
  const plugins = tailwindcss() as Plugin[];
  for (const plugin of plugins) {
    const transform = plugin.transform as { filter?: { id?: { exclude?: RegExp[] } } } | undefined;
    if (plugin.name?.startsWith('@tailwindcss/vite:generate') && transform?.filter?.id) {
      transform.filter.id.exclude = [...(transform.filter.id.exclude ?? []), /\.svelte\?/];
    }
  }
  return plugins;
}

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [...tailwindSkippingSvelteStyles(), sveltekit()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1422,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1423,
        }
      : undefined,
    watch: {
      // 3. ignore the Rust backend, plus directories that hold duplicate copies
      //    of the frontend source: stale git worktrees under `.claude/`, the
      //    direnv/flake source snapshot under `.direnv/`, and `.git`. Watching
      //    those makes Vite fire spurious full page reloads (their duplicate
      //    `src/app.html` etc. change), which reset the in-app navigation.
      ignored: ['**/src-tauri/**', '**/.claude/**', '**/.direnv/**', '**/.git/**'],
    },
  },
}));
