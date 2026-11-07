// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  // Inject component CSS through JavaScript instead of separate virtual CSS modules: the Vite dev
  // server sometimes failed to deliver those when a new window opened ("failed to load virtual
  // css module"), leaving e.g. the overlay's loupe unstyled in the top-left corner.
  vitePlugin: { emitCss: false },
  compilerOptions: { css: "injected" },
  kit: {
    adapter: adapter({
      fallback: "index.html",
    }),
  },
};

export default config;
