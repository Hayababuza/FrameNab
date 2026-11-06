<script lang="ts">
  import { t } from "$lib/i18n/index.svelte";
  // Phase 2: verschiebbarer Aufnahmerahmen, Größenfelder, FPS, Aufnahme/Pause/Stopp, Countdown.
</script>

<main>
  <h1>{t("recorder.title")}</h1>
  <p>{t("recorder.placeholder")}</p>
</main>

<style>
  main {
    height: 100vh;
    display: grid;
    place-content: center;
    text-align: center;
    border: 3px dashed var(--accent);
  }
  h1 {
    margin: 0;
  }
  p {
    color: var(--text-muted);
  }
</style>
