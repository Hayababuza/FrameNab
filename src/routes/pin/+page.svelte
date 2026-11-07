<script lang="ts">
  /** A pinned screenshot: borderless window showing the image; drag to move, scroll to zoom. */
  import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { copyPin, loadPin, pinInfo, runAction, type PinInfo } from "$lib/ipc";

  let canvas: HTMLCanvasElement;
  let info = $state<PinInfo | null>(null);
  let base = { w: 0, h: 0 };
  let zoom = $state(1);
  let flash = $state(false);
  let error = $state<string | null>(null);
  const win = getCurrentWindow();

  onMount(async () => {
    info = await pinInfo();
    if (!info) return void win.close();
    base = { w: window.innerWidth, h: window.innerHeight };
    try {
      const bitmap = await loadPin(info);
      canvas.width = info.width;
      canvas.height = info.height;
      canvas.getContext("2d")!.drawImage(bitmap, 0, 0);
    } catch (e) {
      error = String(e);
    }
  });

  function onmousedown(e: MouseEvent) {
    if (e.button === 0 && e.detail === 1) win.startDragging();
  }

  async function onwheel(e: WheelEvent) {
    e.preventDefault();
    zoom = Math.min(8, Math.max(0.2, zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1)));
    await win.setSize(new LogicalSize(Math.round(base.w * zoom), Math.round(base.h * zoom)));
  }

  /** Hides the controls while a new screenshot is taken, so they are not in the picture. */
  let capturing = $state(false);

  /** New screenshot with this (and every other) pin still open on screen. */
  async function newScreenshot() {
    capturing = true;
    await new Promise((resolve) => setTimeout(resolve, 150));
    await runAction("screenshot");
    setTimeout(() => (capturing = false), 1500);
  }

  async function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") win.close();
    else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "n") {
      e.preventDefault();
      newScreenshot();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "c") {
      await copyPin();
      flash = true;
      setTimeout(() => (flash = false), 250);
    }
  }
</script>

<svelte:window {onkeydown} />

<canvas
  bind:this={canvas}
  title={t("pin.hint")}
  class:flash
  {onmousedown}
  ondblclick={() => win.close()}
  {onwheel}
  oncontextmenu={(e) => {
    e.preventDefault();
    win.close();
  }}
></canvas>

<div class="controls" class:capturing>
  <button title={t("pin.newScreenshot")} aria-label={t("pin.newScreenshot")} onclick={newScreenshot}>
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M4 8h3l2-3h6l2 3h3v11H4zM12 17a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7z" />
    </svg>
  </button>
  <button class="close" title={t("pin.close")} aria-label={t("pin.close")} onclick={() => win.close()}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
  </button>
</div>

{#if error}
  <p class="error">{error}</p>
{/if}

<style>
  .controls {
    position: fixed;
    top: 6px;
    right: 6px;
    display: flex;
    gap: 4px;
    opacity: 0;
    transition: opacity 0.15s;
  }
  :global(body:hover) .controls,
  .controls:focus-within {
    opacity: 1;
  }
  .controls.capturing {
    opacity: 0 !important;
    transition: none;
  }
  .controls button {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: rgb(20 20 24 / 80%);
    color: #fff;
    cursor: pointer;
  }
  .controls button:hover {
    background: rgb(70 70 80 / 95%);
  }
  .controls .close:hover {
    background: #e5484d;
  }
  .controls svg {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .error {
    position: fixed;
    inset: auto 8px 8px;
    margin: 0;
    padding: 6px 8px;
    border-radius: 6px;
    background: rgb(160 30 30 / 90%);
    color: #fff;
    font: 12px sans-serif;
  }
  :global(html),
  :global(body) {
    margin: 0;
    overflow: hidden;
    background: #000;
  }
  canvas {
    display: block;
    width: 100vw;
    height: 100vh;
    cursor: move;
    box-sizing: border-box;
    border: 1px solid rgb(255 99 105 / 70%);
    transition: filter 0.2s;
  }
  canvas.flash {
    filter: brightness(1.4);
  }
</style>
