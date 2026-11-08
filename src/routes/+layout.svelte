<script lang="ts">
  import "../app.css";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, type Snippet } from "svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import { getSetting, type Theme } from "$lib/stores/settings";

  let { children }: { children: Snippet } = $props();

  /** Light/dark for the page and the window frame; "system" follows the OS. */
  function applyTheme(theme: Theme) {
    const root = document.documentElement;
    if (theme === "system") delete root.dataset.theme;
    else root.dataset.theme = theme;
    getCurrentWindow()
      .setTheme(theme === "system" ? null : theme)
      .catch(() => {});
  }

  async function applySettings() {
    i18n.set(await getSetting("locale"));
    applyTheme(await getSetting("theme"));
  }

  onMount(() => {
    applySettings();
    // Settings are changed in a separate window → update every window.
    const unlisten = listen("settings-changed", applySettings);
    return () => void unlisten.then((fn) => fn());
  });
</script>

{@render children()}
