<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { t } from "$lib/i18n/index.svelte";

  // Phase 1: eingefrorenes Bildschirmbild, Bereichsauswahl, Lupe, Annotationswerkzeuge.
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") getCurrentWindow().close();
  }
</script>

<svelte:window {onkeydown} />

<div class="overlay">
  <div class="hint">
    <strong>{t("overlay.hint")}</strong>
    <span>{t("overlay.placeholder")}</span>
  </div>
</div>

<style>
  :global(body) {
    background: transparent;
  }
  .overlay {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 55%);
    cursor: crosshair;
    user-select: none;
  }
  .hint {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 14px 22px;
    border-radius: 10px;
    background: rgb(20 20 24 / 85%);
    color: #fff;
  }
  .hint span {
    opacity: 0.7;
    font-size: 0.9rem;
  }
</style>
