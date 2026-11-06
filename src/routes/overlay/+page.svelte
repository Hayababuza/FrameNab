<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import type { Rect } from "$lib/bindings/Rect";
  import { beginDrag, type Drag, toImagePixels, updateDrag } from "$lib/annotations/selection";
  import { t } from "$lib/i18n/index.svelte";
  import { cancelCapture, captureInfo, copySelection, loadCapture, type CaptureInfo } from "$lib/ipc";

  let canvas: HTMLCanvasElement;
  let info: CaptureInfo | null = null;
  let bitmap: ImageBitmap | null = null;
  let selection = $state<Rect | null>(null);
  let drag: Drag | null = null;
  let busy = $state(false);
  let error = $state<string | null>(null);
  let size = $state({ w: window.innerWidth, h: window.innerHeight });

  /** Image pixels per CSS pixel. */
  const scale = () => (info ? info.width / size.w : 1);

  const imageSelection = $derived.by(() => {
    if (!selection || !info) return null;
    return toImagePixels(selection, info.width / size.w, info);
  });

  onMount(async () => {
    try {
      info = await captureInfo();
      if (!info) return void cancelCapture();
      bitmap = await loadCapture(info);
      draw();
      const win = getCurrentWindow();
      await win.show();
      // Some compositors (GNOME/Wayland) ignore fullscreen requests for hidden windows.
      await win.setFullscreen(true);
      await win.setFocus();
      canvas.focus();
    } catch (e) {
      error = String(e);
      await getCurrentWindow().show();
    }
  });

  function draw() {
    if (!canvas || !bitmap) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(size.w * dpr);
    canvas.height = Math.round(size.h * dpr);
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(bitmap, 0, 0, size.w, size.h);

    // Dim everything except the selection.
    ctx.fillStyle = "rgb(0 0 0 / 50%)";
    ctx.beginPath();
    ctx.rect(0, 0, size.w, size.h);
    if (selection) ctx.rect(selection.x, selection.y, selection.w, selection.h);
    ctx.fill("evenodd");

    if (selection) {
      ctx.strokeStyle = "#ff6369";
      ctx.lineWidth = 1;
      ctx.strokeRect(selection.x + 0.5, selection.y + 0.5, selection.w - 1, selection.h - 1);
    }
  }

  $effect(() => {
    void selection;
    void size;
    draw();
  });

  function point(e: PointerEvent) {
    return { x: e.clientX, y: e.clientY };
  }

  function onpointerdown(e: PointerEvent) {
    if (busy) return;
    if (e.button === 2) return void cancelCapture();
    if (e.button !== 0) return;
    canvas.setPointerCapture(e.pointerId);
    drag = beginDrag(selection, point(e));
  }

  function onpointermove(e: PointerEvent) {
    if (!drag) return;
    selection = updateDrag(drag, point(e), size);
  }

  function onpointerup(e: PointerEvent) {
    if (!drag) return;
    canvas.releasePointerCapture(e.pointerId);
    drag = null;
    // A plain click without dragging leaves no selection.
    if (selection && (selection.w < 2 || selection.h < 2)) selection = null;
  }

  async function copy() {
    if (!info || busy) return;
    busy = true;
    const target = selection ?? { x: 0, y: 0, w: size.w, h: size.h };
    try {
      await copySelection(toImagePixels(target, scale(), info));
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      if (drag) return;
      cancelCapture();
    } else if (e.key === "Enter" || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "c")) {
      e.preventDefault();
      copy();
    }
  }

  function onresize() {
    size = { w: window.innerWidth, h: window.innerHeight };
  }

  // Label next to the selection: above it, or inside if there is no room.
  const labelStyle = $derived.by(() => {
    if (!selection) return "";
    const top = selection.y >= 28 ? selection.y - 26 : selection.y + 4;
    return `left: ${selection.x}px; top: ${top}px;`;
  });
</script>

<svelte:window {onkeydown} {onresize} oncontextmenu={(e) => e.preventDefault()} />

<canvas
  bind:this={canvas}
  tabindex="-1"
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  ondblclick={copy}
  style:width="{size.w}px"
  style:height="{size.h}px"
></canvas>

{#if selection && imageSelection}
  <div class="size" style={labelStyle}>{imageSelection.width} × {imageSelection.height}</div>
{:else if !error}
  <div class="hint">{t("overlay.hint")}</div>
{/if}

{#if error}
  <div class="hint error">{t("overlay.error")}<br /><small>{error}</small></div>
{/if}

<style>
  :global(html),
  :global(body) {
    background: #000;
    overflow: hidden;
  }
  canvas {
    position: fixed;
    inset: 0;
    display: block;
    cursor: crosshair;
    outline: none;
    touch-action: none;
  }
  .hint,
  .size {
    position: fixed;
    pointer-events: none;
    color: #fff;
    background: rgb(20 20 24 / 85%);
    border-radius: 6px;
    font-size: 13px;
  }
  .hint {
    top: 24px;
    left: 50%;
    transform: translateX(-50%);
    padding: 8px 16px;
    text-align: center;
  }
  .size {
    padding: 2px 8px;
    font-variant-numeric: tabular-nums;
  }
  .error {
    top: 50%;
    background: rgb(160 30 30 / 90%);
  }
</style>
