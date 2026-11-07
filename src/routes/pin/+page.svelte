<script lang="ts">
  /** A pinned screenshot: borderless window showing the image; drag to move, scroll to zoom. */
  import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { copyPin, loadPin, pinInfo, type PinInfo } from "$lib/ipc";

  let canvas: HTMLCanvasElement;
  let info = $state<PinInfo | null>(null);
  let base = { w: 0, h: 0 };
  let zoom = $state(1);
  let flash = $state(false);
  const win = getCurrentWindow();

  onMount(async () => {
    info = await pinInfo();
    if (!info) return void win.close();
    base = { w: window.innerWidth, h: window.innerHeight };
    const bitmap = await loadPin(info);
    canvas.width = info.width;
    canvas.height = info.height;
    canvas.getContext("2d")!.drawImage(bitmap, 0, 0);
  });

  function onmousedown(e: MouseEvent) {
    if (e.button === 0 && e.detail === 1) win.startDragging();
  }

  async function onwheel(e: WheelEvent) {
    e.preventDefault();
    zoom = Math.min(8, Math.max(0.2, zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1)));
    await win.setSize(new LogicalSize(Math.round(base.w * zoom), Math.round(base.h * zoom)));
  }

  async function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") win.close();
    else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "c") {
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

<style>
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
