import { defineConfig } from 'vitest/config';
import { sveltekit } from '@sveltejs/kit/vite';
import { resolve } from 'path';

export default defineConfig({
  plugins: [sveltekit()],
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['src/test/setup.ts'],
    include: ['src/**/*.{test,spec}.{js,ts}'],
  },
  resolve: {
    conditions: ['browser'],
    alias: {
      $components: resolve(import.meta.dirname, 'src/lib/components'),
      $stores: resolve(import.meta.dirname, 'src/lib/stores'),
      $api: resolve(import.meta.dirname, 'src/lib/api'),
    },
  },
});
