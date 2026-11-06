<script lang="ts">
  import "../app.css";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, type Snippet } from "svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import { getSetting } from "$lib/stores/settings";

  let { children }: { children: Snippet } = $props();

  onMount(() => {
    getSetting("locale").then((l) => i18n.set(l));
    // Settings are changed in a separate window → update every window.
    const unlisten = listen<{ locale: typeof i18n.locale | null }>("settings-changed", (e) =>
      i18n.set(e.payload.locale),
    );
    return () => void unlisten.then((fn) => fn());
  });
</script>

{@render children()}
