<script lang="ts">
  /**
   * Pixel loupe for precise selection: an enlarged view of the pixels around the cursor with
   * the image coordinates and the color under the cursor.
   */
  import type { Point } from "$lib/bindings/Point";

  interface Props {
    /** RGBA pixels of the screenshot. */
    pixels: Uint8ClampedArray;
    imageWidth: number;
    imageHeight: number;
    /** Position in image pixels. */
    at: Point;
    /** Cursor position in CSS pixels (where the loupe is shown). */
    cursor: Point;
    screen: { w: number; h: number };
  }

  let { pixels, imageWidth, imageHeight, at, cursor, screen }: Props = $props();

  const RADIUS = 7; // pixels shown on each side of the center
  const ZOOM = 9;
  const SIZE = (RADIUS * 2 + 1) * ZOOM;
  const OFFSET = 24;

  let canvas: HTMLCanvasElement;

  const px = $derived(Math.min(imageWidth - 1, Math.max(0, Math.floor(at.x))));
  const py = $derived(Math.min(imageHeight - 1, Math.max(0, Math.floor(at.y))));

  function rgbAt(x: number, y: number): [number, number, number] | null {
    if (x < 0 || y < 0 || x >= imageWidth || y >= imageHeight) return null;
    const i = (y * imageWidth + x) * 4;
    return [pixels[i], pixels[i + 1], pixels[i + 2]];
  }

  const hex = $derived.by(() => {
    const c = rgbAt(px, py) ?? [0, 0, 0];
    return (
      "#" +
      c
        .map((v) => v.toString(16).padStart(2, "0"))
        .join("")
        .toUpperCase()
    );
  });

  // Keep the loupe on screen: flip to the other side of the cursor near the edges.
  const style = $derived.by(() => {
    const boxH = SIZE + 40;
    const left = cursor.x + OFFSET + SIZE > screen.w ? cursor.x - OFFSET - SIZE : cursor.x + OFFSET;
    const top = cursor.y + OFFSET + boxH > screen.h ? cursor.y - OFFSET - boxH : cursor.y + OFFSET;
    // position is set inline as well, so the loupe is placed correctly even without its CSS.
    return `position: fixed; left: ${left}px; top: ${top}px;`;
  });

  $effect(() => {
    if (!canvas) return;
    const ctx = canvas.getContext("2d")!;
    ctx.fillStyle = "#000";
    ctx.fillRect(0, 0, SIZE, SIZE);
    for (let dy = -RADIUS; dy <= RADIUS; dy++) {
      for (let dx = -RADIUS; dx <= RADIUS; dx++) {
        const c = rgbAt(px + dx, py + dy);
        if (!c) continue;
        ctx.fillStyle = `rgb(${c[0]} ${c[1]} ${c[2]})`;
        ctx.fillRect((dx + RADIUS) * ZOOM, (dy + RADIUS) * ZOOM, ZOOM, ZOOM);
      }
    }
    // Crosshair through the center pixel.
    ctx.fillStyle = "rgb(255 99 105 / 35%)";
    ctx.fillRect(RADIUS * ZOOM, 0, ZOOM, SIZE);
    ctx.fillRect(0, RADIUS * ZOOM, SIZE, ZOOM);
    ctx.strokeStyle = "#fff";
    ctx.lineWidth = 1;
    ctx.strokeRect(RADIUS * ZOOM + 0.5, RADIUS * ZOOM + 0.5, ZOOM - 1, ZOOM - 1);
  });
</script>

<div class="loupe" {style}>
  <canvas bind:this={canvas} width={SIZE} height={SIZE}></canvas>
  <div class="info">
    <span>{px}, {py}</span>
    <span class="color"><i style:background={hex}></i>{hex}</span>
  </div>
</div>

<style>
  .loupe {
    position: fixed;
    pointer-events: none;
    border-radius: 8px;
    overflow: hidden;
    background: rgb(20 20 24 / 94%);
    box-shadow: 0 4px 16px rgb(0 0 0 / 50%);
    border: 1px solid rgb(255 255 255 / 25%);
    color: #fff;
    font:
      12px/1 ui-monospace,
      monospace;
  }
  canvas {
    display: block;
    image-rendering: pixelated;
  }
  .info {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 8px;
  }
  .color {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  i {
    width: 10px;
    height: 10px;
    border-radius: 2px;
    border: 1px solid rgb(255 255 255 / 50%);
  }
</style>
