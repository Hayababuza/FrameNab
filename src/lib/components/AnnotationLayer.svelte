<script lang="ts">
  /**
   * Editor preview: draws the current frame and the annotations visible on it, and lets the user
   * draw, select, move and type annotations. Annotations are in image pixels of the recording.
   * New annotations cover the frames in `range`; every change goes to `onchange`.
   */
  import { onMount, tick } from "svelte";
  import type { Annotation } from "$lib/bindings/Annotation";
  import type { Point } from "$lib/bindings/Point";
  import type { Rect } from "$lib/bindings/Rect";
  import type { Rgba } from "$lib/bindings/Rgba";
  import { hitTest, LINE_HEIGHT, translateShape } from "$lib/annotations/geometry";
  import { drawAnnotation, drawSelectionFrame, FONT_FAMILY } from "$lib/annotations/render";
  import {
    fontSizeFor,
    isDragTool,
    isTrivial,
    nextCounterValue,
    shapeFromDrag,
    type Tool,
  } from "$lib/annotations/tools";
  import { t } from "$lib/i18n/index.svelte";

  interface Props {
    /** URL of the current frame's image. */
    src: string;
    width: number;
    height: number;
    annotations: Annotation[];
    /** Index of the shown frame. */
    frame: number;
    /** Frames that new annotations cover. */
    range: [number, number];
    tool: Tool;
    color: Rgba;
    strokeWidth: number;
    selectedId: string | null;
    /** Editing is off (e.g. while playing). */
    readonly?: boolean;
    /** Drawn on top, not editable (input effects). */
    extra?: Annotation[];
    /** Crop mode: the area to keep (image pixels); dragging edits it instead of annotations. */
    crop?: Rect | null;
    onchange: (annotations: Annotation[]) => void;
  }

  let {
    src,
    width,
    height,
    annotations,
    frame,
    range,
    tool,
    color,
    strokeWidth,
    selectedId = $bindable(),
    readonly = false,
    extra = [],
    crop = $bindable(null),
    onchange,
  }: Props = $props();

  let canvas: HTMLCanvasElement;
  let textEl: HTMLDivElement | undefined = $state();
  let box = $state({ w: 0, h: 0 });
  let image: HTMLImageElement | null = $state(null);
  let draft = $state<Annotation | null>(null);
  let drag:
    | { kind: "draw"; start: Point; points: Point[] }
    | { kind: "move"; id: string; last: Point; moved: boolean }
    | { kind: "crop"; mode: "new" | "move" | "corner"; anchor: Point; start: Rect; from: Point }
    | null = null;
  /** While dragging an annotation, its moved copy (committed on release). */
  let moving = $state<Annotation[] | null>(null);
  let textEdit = $state<{ at: Point; fontSize: number; value: string } | null>(null);
  let seq = 0;

  /** Where the frame is drawn: fitted into the box, centered. `k` = CSS pixels per image pixel. */
  const fit = $derived.by(() => {
    const k = Math.min(box.w / Math.max(1, width), box.h / Math.max(1, height));
    return { k, x: (box.w - width * k) / 2, y: (box.h - height * k) / 2 };
  });

  const shown = $derived((moving ?? annotations).filter((a) => a.frameStart <= frame && frame <= a.frameEnd));

  // Load the frame image; keep showing the previous one until the next has loaded (no flicker).
  $effect(() => {
    const next = new Image();
    next.onload = () => (image = next);
    next.src = src;
  });

  $effect(() => {
    void [image, shown, draft, selectedId, fit, box, extra, crop?.x, crop?.y, crop?.w, crop?.h];
    draw();
  });

  function draw() {
    if (!canvas || !box.w || !box.h) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(box.w * dpr);
    canvas.height = Math.round(box.h * dpr);
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, box.w, box.h);
    if (image) {
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(image, fit.x, fit.y, width * fit.k, height * fit.k);
    }
    ctx.save();
    ctx.setTransform(dpr * fit.k, 0, 0, dpr * fit.k, dpr * fit.x, dpr * fit.y);
    for (const a of shown) drawAnnotation(ctx, a);
    for (const a of extra) drawAnnotation(ctx, a);
    if (draft) drawAnnotation(ctx, draft);
    const selected = shown.find((a) => a.id === selectedId);
    if (selected && !crop) drawSelectionFrame(ctx, selected, 1 / fit.k);
    ctx.restore();
    if (crop) drawCrop(ctx, crop);
  }

  /** Dims everything outside the crop area and draws its frame, corner handles and size. */
  function drawCrop(ctx: CanvasRenderingContext2D, r: Rect) {
    const x = fit.x + r.x * fit.k;
    const y = fit.y + r.y * fit.k;
    const w = r.w * fit.k;
    const h = r.h * fit.k;
    ctx.fillStyle = "rgba(0, 0, 0, 0.55)";
    ctx.beginPath();
    ctx.rect(0, 0, box.w, box.h);
    ctx.rect(x, y, w, h);
    ctx.fill("evenodd");
    ctx.strokeStyle = "rgba(255, 255, 255, 0.95)";
    ctx.lineWidth = 1;
    ctx.setLineDash([6, 4]);
    ctx.strokeRect(x + 0.5, y + 0.5, w, h);
    ctx.setLineDash([]);
    ctx.fillStyle = "#fff";
    for (const [cx, cy] of corners(r)) {
      ctx.fillRect(fit.x + cx * fit.k - 5, fit.y + cy * fit.k - 5, 10, 10);
    }
    const label = `${Math.round(r.w)} × ${Math.round(r.h)}`;
    ctx.font = "12px sans-serif";
    const lw = ctx.measureText(label).width + 10;
    const ly = y > 24 ? y - 22 : y + 4;
    ctx.fillStyle = "rgba(0, 0, 0, 0.7)";
    ctx.fillRect(x, ly, lw, 18);
    ctx.fillStyle = "#fff";
    ctx.fillText(label, x + 5, ly + 13);
  }

  const corners = (r: Rect): [number, number][] => [
    [r.x, r.y],
    [r.x + r.w, r.y],
    [r.x, r.y + r.h],
    [r.x + r.w, r.y + r.h],
  ];

  const clampX = (v: number) => Math.max(0, Math.min(width, v));
  const clampY = (v: number) => Math.max(0, Math.min(height, v));

  /** Rect spanned by two points, inside the image, in whole pixels. */
  function span(a: Point, b: Point): Rect {
    const x0 = Math.round(clampX(Math.min(a.x, b.x)));
    const y0 = Math.round(clampY(Math.min(a.y, b.y)));
    const x1 = Math.round(clampX(Math.max(a.x, b.x)));
    const y1 = Math.round(clampY(Math.max(a.y, b.y)));
    return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
  }

  function cropDown(p: Point) {
    if (!crop) return;
    const near = 10 / fit.k;
    const corner = corners(crop).findIndex(
      ([cx, cy]) => Math.abs(p.x - cx) < near && Math.abs(p.y - cy) < near,
    );
    if (corner >= 0) {
      // The opposite corner stays where it is.
      const [ox, oy] = corners(crop)[3 - corner];
      drag = { kind: "crop", mode: "corner", anchor: { x: ox, y: oy }, start: { ...crop }, from: p };
    } else if (p.x > crop.x && p.x < crop.x + crop.w && p.y > crop.y && p.y < crop.y + crop.h) {
      drag = { kind: "crop", mode: "move", anchor: p, start: { ...crop }, from: p };
    } else {
      drag = { kind: "crop", mode: "new", anchor: p, start: { ...crop }, from: p };
    }
  }

  function cropMove(p: Point) {
    if (drag?.kind !== "crop") return;
    if (drag.mode === "move") {
      const { start } = drag;
      const x = Math.round(Math.max(0, Math.min(width - start.w, start.x + p.x - drag.from.x)));
      const y = Math.round(Math.max(0, Math.min(height - start.h, start.y + p.y - drag.from.y)));
      crop = { ...start, x, y };
    } else {
      const next = span(drag.anchor, p);
      if (next.w >= 1 && next.h >= 1) crop = next;
    }
  }

  onMount(() => draw());

  // ---- helpers -------------------------------------------------------------------------------

  function toImage(e: PointerEvent | MouseEvent): Point {
    const r = canvas.getBoundingClientRect();
    return { x: (e.clientX - r.left - fit.x) / fit.k, y: (e.clientY - r.top - fit.y) / fit.k };
  }

  function create(shape: Annotation["shape"], style?: Annotation["style"]): Annotation {
    return {
      id: `a${Date.now().toString(36)}${++seq}`,
      frameStart: range[0],
      frameEnd: range[1],
      shape,
      style: style ?? { stroke: { ...color }, width: strokeWidth, fill: null, opacity: 1 },
    };
  }

  function annotationAt(p: Point) {
    const tolerance = 6 / fit.k;
    return shown.findLast((a) => hitTest(a.shape, p, tolerance + a.style.width / 2, a.style.width));
  }

  // ---- pointer -------------------------------------------------------------------------------

  function onpointerdown(e: PointerEvent) {
    if (readonly || e.button !== 0) return;
    if (textEdit) return commitText();
    const p = toImage(e);
    canvas.setPointerCapture(e.pointerId);
    if (crop) {
      cropDown(p);
    } else if (tool === "select") {
      const hit = annotationAt(p);
      selectedId = hit?.id ?? null;
      if (hit) drag = { kind: "move", id: hit.id, last: p, moved: false };
    } else if (tool === "text") {
      textEdit = { at: p, fontSize: fontSizeFor(strokeWidth), value: "" };
    } else if (tool === "counter") {
      onchange([...annotations, create({ type: "counter", at: p, value: nextCounterValue(annotations) })]);
    } else if (tool === "click") {
      onchange([...annotations, create({ type: "click", at: p })]);
    } else if (isDragTool(tool)) {
      drag = { kind: "draw", start: p, points: [p] };
    }
  }

  function onpointermove(e: PointerEvent) {
    if (!drag) return;
    const p = toImage(e);
    if (drag.kind === "crop") {
      cropMove(p);
    } else if (drag.kind === "draw") {
      drag.points.push(p);
      const shape = shapeFromDrag(tool, drag.start, p, [...drag.points], strokeWidth);
      draft = shape ? create(shape) : null;
    } else {
      const { id, last } = drag;
      const dx = p.x - last.x;
      const dy = p.y - last.y;
      moving = (moving ?? annotations).map((a) =>
        a.id === id ? { ...a, shape: translateShape(a.shape, dx, dy) } : a,
      );
      drag.last = p;
      drag.moved = true;
    }
  }

  function onpointerup(e: PointerEvent) {
    if (!drag) return;
    if (canvas.hasPointerCapture(e.pointerId)) canvas.releasePointerCapture(e.pointerId);
    if (drag.kind === "draw" && draft && !isTrivial(draft.shape, 3)) onchange([...annotations, draft]);
    if (drag.kind === "move" && drag.moved && moving) onchange(moving);
    draft = null;
    moving = null;
    drag = null;
  }

  function ondblclick(e: MouseEvent) {
    // Double-click a text to edit it.
    const hit = tool === "select" && !crop ? annotationAt(toImage(e)) : undefined;
    if (hit?.shape.type !== "text") return;
    onchange(annotations.filter((a) => a.id !== hit.id));
    selectedId = null;
    textEdit = { at: hit.shape.at, fontSize: hit.shape.fontSize, value: hit.shape.text };
  }

  // ---- text ----------------------------------------------------------------------------------

  $effect(() => {
    if (textEdit && textEl && document.activeElement !== textEl) tick().then(() => textEl?.focus());
  });

  function commitText() {
    if (!textEdit) return;
    const text = (textEl?.innerText ?? textEdit.value).replace(/\n$/, "");
    const { at, fontSize } = textEdit;
    textEdit = null;
    if (text.trim())
      onchange([...annotations, create({ type: "text", at, text, fontSize, background: null })]);
  }

  function ontextkey(e: KeyboardEvent) {
    e.stopPropagation(); // keep editor shortcuts (Del, Space, arrows…) out of the text field
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      commitText();
    } else if (e.key === "Escape") {
      e.preventDefault();
      textEdit = null;
    }
  }

  const cursor = $derived(
    crop
      ? "crosshair"
      : readonly
        ? "default"
        : tool === "select"
          ? "default"
          : tool === "text"
            ? "text"
            : "crosshair",
  );
</script>

<div class="layer" bind:clientWidth={box.w} bind:clientHeight={box.h}>
  <canvas
    bind:this={canvas}
    style:width="{box.w}px"
    style:height="{box.h}px"
    style:cursor
    {onpointerdown}
    {onpointermove}
    {onpointerup}
    {ondblclick}
  ></canvas>
  {#if textEdit}
    <div
      class="text-edit"
      bind:this={textEl}
      contenteditable="plaintext-only"
      role="textbox"
      tabindex="0"
      aria-label={t("tool.text")}
      data-placeholder={t("overlay.textPlaceholder")}
      onkeydown={ontextkey}
      style:left="{fit.x + textEdit.at.x * fit.k}px"
      style:top="{fit.y + textEdit.at.y * fit.k}px"
      style:font="{textEdit.fontSize * fit.k}px/{LINE_HEIGHT}
      {FONT_FAMILY}"
      style:color="rgb({color.r}
      {color.g}
      {color.b})"
    >
      {textEdit.value}
    </div>
  {/if}
</div>

<style>
  .layer {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
  canvas {
    position: absolute;
    inset: 0;
    display: block;
    touch-action: none;
  }
  .text-edit {
    position: absolute;
    min-width: 1em;
    white-space: pre;
    outline: 1px dashed currentColor;
    outline-offset: 2px;
  }
  .text-edit:empty::before {
    content: attr(data-placeholder);
    font-size: 12px;
    color: var(--text-muted);
  }
</style>
