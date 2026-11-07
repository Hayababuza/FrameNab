<script lang="ts">
  /**
   * A pinned screenshot: borderless window showing the image. Drag to move; resize with the
   * handles (shown on hover) or the mouse wheel. The window keeps the image's aspect ratio.
   */
  import { getCurrentWindow, LogicalSize, type Window } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { copyPin, loadPin, pinInfo, runAction, type PinInfo } from "$lib/ipc";

  let canvas: HTMLCanvasElement;
  let info = $state<PinInfo | null>(null);
  /** Width / height of the image. */
  let aspect = 1;
  let snapTimer: ReturnType<typeof setTimeout> | undefined;
  let flash = $state(false);
  let error = $state<string | null>(null);
  const win = getCurrentWindow();
  type ResizeDirection = Parameters<Window["startResizeDragging"]>[0];

  onMount(async () => {
    info = await pinInfo();
    if (!info) return void win.close();
    aspect = info.width / info.height;
    settled = { w: window.innerWidth, h: window.innerHeight };
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

  const MIN_SIZE = 40;

  /** Resizes the window to `width`, with the height following the image's aspect ratio. */
  function setWidth(width: number) {
    const w = Math.max(MIN_SIZE, Math.round(width));
    return win.setSize(new LogicalSize(w, Math.max(MIN_SIZE, Math.round(w / aspect))));
  }

  async function onwheel(e: WheelEvent) {
    e.preventDefault();
    await setWidth(window.innerWidth * (e.deltaY < 0 ? 1.1 : 1 / 1.1));
  }

  // Edges and corners for resizing (useful without a mouse wheel).
  const handles: { dir: ResizeDirection; cls: string }[] = [
    { dir: "NorthWest", cls: "nw" },
    { dir: "North", cls: "n" },
    { dir: "NorthEast", cls: "ne" },
    { dir: "East", cls: "e" },
    { dir: "SouthEast", cls: "se" },
    { dir: "South", cls: "s" },
    { dir: "SouthWest", cls: "sw" },
    { dir: "West", cls: "w" },
  ];

  function startResize(e: MouseEvent, dir: ResizeDirection) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    win.startResizeDragging(dir);
  }

  /** Window size the last time it matched the image's aspect ratio. */
  let settled = { w: 0, h: 0 };

  /**
   * After a free resize (handles) the window may no longer match the image; once the size has
   * settled, snap it back: the dimension the user changed more wins, the other one follows.
   */
  function onresize() {
    clearTimeout(snapTimer);
    snapTimer = setTimeout(() => {
      const { innerWidth: w, innerHeight: h } = window;
      if (Math.abs(w - h * aspect) <= 2) {
        settled = { w, h };
        return;
      }
      if (Math.abs(w - settled.w) >= Math.abs(h - settled.h)) setWidth(w);
      else setWidth(h * aspect);
    }, 300);
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

<svelte:window {onkeydown} {onresize} />

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

<div class="handles" class:capturing>
  {#each handles as h (h.dir)}
    <span class="handle {h.cls}" role="presentation" onmousedown={(e) => startResize(e, h.dir)}></span>
  {/each}
</div>

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
  .handles {
    position: fixed;
    inset: 0;
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.15s;
  }
  :global(body:hover) .handles {
    opacity: 1;
  }
  .handles.capturing {
    opacity: 0 !important;
    transition: none;
  }
  .handle {
    position: absolute;
    width: 10px;
    height: 10px;
    margin: -5px 0 0 -5px;
    border: 1px solid #e5484d;
    border-radius: 2px;
    background: #fff;
    box-shadow: 0 0 0 1px rgb(0 0 0 / 30%);
    pointer-events: auto;
  }
  .nw {
    left: 5px;
    top: 5px;
    cursor: nwse-resize;
  }
  .n {
    left: 50%;
    top: 5px;
    cursor: ns-resize;
  }
  .ne {
    left: calc(100% - 5px);
    top: 5px;
    cursor: nesw-resize;
  }
  .e {
    left: calc(100% - 5px);
    top: 50%;
    cursor: ew-resize;
  }
  .se {
    left: calc(100% - 5px);
    top: calc(100% - 5px);
    cursor: nwse-resize;
  }
  .s {
    left: 50%;
    top: calc(100% - 5px);
    cursor: ns-resize;
  }
  .sw {
    left: 5px;
    top: calc(100% - 5px);
    cursor: nesw-resize;
  }
  .w {
    left: 5px;
    top: 50%;
    cursor: ew-resize;
  }
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
    object-fit: contain;
    cursor: move;
    box-sizing: border-box;
    border: 1px solid rgb(255 99 105 / 70%);
    transition: filter 0.2s;
  }
  canvas.flash {
    filter: brightness(1.4);
  }
</style>
