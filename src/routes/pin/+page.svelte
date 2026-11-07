<script lang="ts">
  /**
   * A pinned screenshot: a borderless, transparent window showing the image with a frame and
   * resize handles on it (like the overlay selection). Drag to move; resize with the handles or
   * the mouse wheel. The window keeps the image's aspect ratio.
   */
  import { getCurrentWindow, LogicalSize, type Window } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { type Handle, handlePoint } from "$lib/annotations/selection";
  import { t } from "$lib/i18n/index.svelte";
  import { copyPin, loadPin, pinInfo, runAction, type PinInfo } from "$lib/ipc";

  type ResizeDirection = Parameters<Window["startResizeDragging"]>[0];

  /** Transparent space around the image; keep in sync with `MARGIN` in `src-tauri/src/pin.rs`. */
  const MARGIN = 7;
  const MIN_IMAGE = 32;

  /** Only corner handles: with a fixed aspect ratio, edge handles would change both sides anyway. */
  const corners: Handle[] = ["nw", "ne", "se", "sw"];

  const directions: Record<Handle, ResizeDirection> = {
    nw: "NorthWest",
    n: "North",
    ne: "NorthEast",
    e: "East",
    se: "SouthEast",
    s: "South",
    sw: "SouthWest",
    w: "West",
  };
  const cursors: Record<Handle, string> = {
    nw: "nwse-resize",
    se: "nwse-resize",
    ne: "nesw-resize",
    sw: "nesw-resize",
    n: "ns-resize",
    s: "ns-resize",
    e: "ew-resize",
    w: "ew-resize",
  };

  const win = getCurrentWindow();
  let canvas: HTMLCanvasElement;
  let info = $state<PinInfo | null>(null);
  let flash = $state(false);
  let error = $state<string | null>(null);
  /** Hides handles and controls while a new screenshot is taken, so they are not in the picture. */
  let capturing = $state(false);

  /** Width / height of the image. */
  let aspect = $state(1);
  let view = $state({ w: window.innerWidth, h: window.innerHeight });

  /** Where the image is drawn: as large as fits inside the margins, centered. */
  const rect = $derived.by(() => {
    const aw = Math.max(1, view.w - 2 * MARGIN);
    const ah = Math.max(1, view.h - 2 * MARGIN);
    const w = Math.min(aw, ah * aspect);
    const h = w / aspect;
    return { x: (view.w - w) / 2, y: (view.h - h) / 2, w, h };
  });

  onMount(async () => {
    info = await pinInfo();
    if (!info) return void win.close();
    aspect = info.width / info.height;
    settled = { ...view };
    try {
      const bitmap = await loadPin(info);
      canvas.width = info.width;
      canvas.height = info.height;
      canvas.getContext("2d")!.drawImage(bitmap, 0, 0);
    } catch (e) {
      error = String(e);
    }
  });

  // ---- moving and resizing -------------------------------------------------------------------

  function onmousedown(e: MouseEvent) {
    if (e.button === 0 && e.detail === 1) win.startDragging();
  }

  /** Resizes the window so the image is `width` wide. */
  function setImageWidth(width: number) {
    const w = Math.max(MIN_IMAGE, width);
    return win.setSize(new LogicalSize(Math.round(w + 2 * MARGIN), Math.round(w / aspect + 2 * MARGIN)));
  }

  async function onwheel(e: WheelEvent) {
    e.preventDefault();
    await setImageWidth(rect.w * (e.deltaY < 0 ? 1.1 : 1 / 1.1));
  }

  function startResize(e: MouseEvent, handle: Handle) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    win.startResizeDragging(directions[handle]);
  }

  /** Window size the last time the image filled it. */
  let settled = { w: 0, h: 0 };
  let snapTimer: ReturnType<typeof setTimeout> | undefined;

  function onresize() {
    view = { w: window.innerWidth, h: window.innerHeight };
    // Where the window manager keeps the aspect ratio (Linux), there is nothing to fix. Elsewhere
    // trim the window to the image once the size has settled: the dimension the user changed
    // more is kept and the other follows.
    if (!info || info.aspectLocked) return;
    clearTimeout(snapTimer);
    snapTimer = setTimeout(() => {
      const aw = view.w - 2 * MARGIN;
      const ah = view.h - 2 * MARGIN;
      if (Math.abs(aw - ah * aspect) <= 2) {
        settled = { ...view };
        return;
      }
      if (Math.abs(view.w - settled.w) >= Math.abs(view.h - settled.h)) setImageWidth(aw);
      else setImageWidth(ah * aspect);
    }, 400);
  }

  // ---- actions -------------------------------------------------------------------------------

  /** New screenshot with this (and every other) pin still open on screen. */
  async function newScreenshot() {
    capturing = true;
    await new Promise((resolve) => setTimeout(resolve, 150));
    await runAction("screenshot");
    setTimeout(() => (capturing = false), 1500);
  }

  async function onkeydown(e: KeyboardEvent) {
    const ctrl = e.ctrlKey || e.metaKey;
    if (e.key === "Escape") win.close();
    else if (ctrl && e.key.toLowerCase() === "n") {
      e.preventDefault();
      newScreenshot();
    } else if (ctrl && e.key.toLowerCase() === "c") {
      await copyPin();
      flash = true;
      setTimeout(() => (flash = false), 250);
    }
  }

  const rectStyle = $derived(`left: ${rect.x}px; top: ${rect.y}px; width: ${rect.w}px; height: ${rect.h}px;`);
</script>

<svelte:window {onkeydown} {onresize} />

<canvas
  bind:this={canvas}
  title={t("pin.hint")}
  class:flash
  style={rectStyle}
  {onmousedown}
  ondblclick={() => win.close()}
  {onwheel}
  oncontextmenu={(e) => {
    e.preventDefault();
    win.close();
  }}
></canvas>

{#if !capturing}
  {#each corners as h (h)}
    {@const c = handlePoint(rect, h)}
    <span
      class="handle"
      role="presentation"
      style="left: {c.x}px; top: {c.y}px; cursor: {cursors[h]};"
      onmousedown={(e) => startResize(e, h)}
    ></span>
  {/each}
{/if}

<div class="controls" class:capturing style="top: {rect.y + 6}px; right: {view.w - rect.x - rect.w + 6}px;">
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
  :global(html),
  :global(body) {
    margin: 0;
    overflow: hidden;
    background: transparent;
  }
  canvas {
    position: fixed;
    display: block;
    cursor: move;
    outline: 1px solid #ff6369;
    transition: filter 0.2s;
  }
  canvas.flash {
    filter: brightness(1.4);
  }
  /* Same look as the handles of the overlay selection: centered on the frame line. */
  .handle {
    position: fixed;
    width: 7px;
    height: 7px;
    margin: -4px 0 0 -4px;
    border: 1px solid #ff6369;
    background: #fff;
  }
  .controls {
    position: fixed;
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
</style>
