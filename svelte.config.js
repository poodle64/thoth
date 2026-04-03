import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      fallback: 'index.html',
    }),
    alias: {
      '$components': 'src/lib/components',
      '$stores': 'src/lib/stores',
      '$api': 'src/lib/api',
    },
  },
  onwarn: (warning, handler) => {
    if (warning.code === 'a11y_click_events_have_key_events') {
      return;
    }
    handler(warning);
  },
};

export default config;
