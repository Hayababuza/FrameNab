<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import type { Annotation } from "$lib/bindings/Annotation";
  import type { Point } from "$lib/bindings/Point";
  import type { Rect } from "$lib/bindings/Rect";
  import type { Rgba } from "$lib/bindings/Rgba";
  import { hitTest, LINE_HEIGHT, translateShape } from "$lib/annotations/geometry";
  import { drawAnnotation, drawSelectionFrame, FONT_FAMILY } from "$lib/annotations/render";
  import {
    beginDrag,
    type Drag,
    type Handle,
    handleAt,
    handleCursor,
    handlePoint,
    handles,
    toImagePixels,
    toolbarPosition,
    updateDrag,
    windowAt,
  } from "$lib/annotations/selection";
  import {
    colors,
    fontSizeFor,
    fromHex,
    isDragTool,
    isTrivial,
    nextCounterValue,
    shapeFromDrag,
    toHex,
    type Tool,
    toolForKey,
    tools,
    widths,
  } from "$lib/annotations/tools";
  import Magnifier from "$lib/components/Magnifier.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import {
    cancelCapture,
    captureInfo,
    copySelection,
    defaultSaveDirectory,
    loadCapture,
    pinSelection,
    recordSelection,
    recordWindow,
    saveSelection,
    type CaptureInfo,
    focusWindow,
  } from "$lib/ipc";
  import { History } from "$lib/stores/history";
  import { getSetting } from "$lib/stores/settings";

  type Interaction =
    | { kind: "area"; drag: Drag }
    | { kind: "draw"; start: Point; points: Point[] }
    | { kind: "move"; id: string; last: Point; recorded: boolean };

  let canvas: HTMLCanvasElement;
  let textEl: HTMLDivElement | undefined = $state();
  let info = $state<CaptureInfo | null>(null);
  let bitmap: ImageBitmap | null = null;
  let pixels = $state<Uint8ClampedArray | null>(null);
  /** Cursor in CSS pixels, while it is over the overlay. */
  let pointer = $state<Point | null>(null);
  let hoverHandle = $state<Handle | null>(null);
  /** Window under the cursor when the area drag started; a click without dragging selects it. */
  let clickedWindow: Rect | null = null;
  let clickPending = $state(false);

  let size = $state({ w: window.innerWidth, h: window.innerHeight });
  /** Area to copy, in CSS pixels. */
  let selection = $state<Rect | null>(null);
  let interaction = $state<Interaction | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let toolbarSize = $state({ w: 0, h: 0 });
  let hovering = $state(false);

  let tool = $state<Tool>("select");
  let color = $state<Rgba>(colors[0]);
  let width = $state(widths[1]);
  /** Annotations in image pixels. */
  let annotations = $state<Annotation[]>([]);
  let draft = $state<Annotation | null>(null);
  let selectedId = $state<string | null>(null);
  let textEdit = $state<{ at: Point; value: string; style: Annotation["style"]; fontSize: number } | null>(
    null,
  );
  const history = new History<Annotation[]>(200);
  let historyVersion = $state(0);
  let nextId = 0;

  /** Image pixels per CSS pixel. */
  let scale = $state(1);
  const toImage = (p: Point): Point => ({ x: p.x * scale, y: p.y * scale });

  const imageSelection = $derived(selection && info ? toImagePixels(selection, scale, info) : null);

  /** Target folder of "Save", shown in its tooltip. */
  let saveDirectory = $state("");

  onMount(async () => {
    Promise.all([getSetting("saveDirectory"), defaultSaveDirectory()]).then(
      ([custom, fallback]) => (saveDirectory = custom ?? fallback),
    );
    try {
      info = await captureInfo();
      if (!info) return void cancelCapture();
      scale = info.width / size.w;
      const loaded = await loadCapture(info);
      bitmap = loaded.bitmap;
      pixels = loaded.pixels;
      draw();
      const win = getCurrentWindow();
      await win.show();
      // Some compositors (GNOME/Wayland) ignore fullscreen requests for hidden windows. A window
      // across all monitors is already sized; fullscreen would shrink it to one monitor.
      if (!info.span) await win.setFullscreen(true);
      await focusWindow();
      canvas.focus();
    } catch (e) {
      error = String(e);
      await getCurrentWindow().show();
    }
  });

  function draw() {
    if (!canvas || !bitmap || !info) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(size.w * dpr);
    canvas.height = Math.round(size.h * dpr);
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(bitmap, 0, 0, size.w, size.h);

    // Annotations are in image pixels.
    ctx.save();
    ctx.setTransform(dpr / scale, 0, 0, dpr / scale, 0, 0);
    for (const a of annotations) drawAnnotation(ctx, a);
    if (draft) drawAnnotation(ctx, draft);
    const selected = annotations.find((a) => a.id === selectedId);
    if (selected) drawSelectionFrame(ctx, selected, scale);
    ctx.restore();

    // Window snapping: outline the window under the cursor while there is no selection yet.
    if (!selection && hoverWindow) {
      ctx.fillStyle = "rgb(255 99 105 / 12%)";
      ctx.fillRect(hoverWindow.x, hoverWindow.y, hoverWindow.w, hoverWindow.h);
      ctx.strokeStyle = "#ff6369";
      ctx.lineWidth = 2;
      ctx.strokeRect(hoverWindow.x + 1, hoverWindow.y + 1, hoverWindow.w - 2, hoverWindow.h - 2);
    }

    // Dim everything except the selection.
    ctx.fillStyle = "rgba(0, 0, 0, 0.5)";
    ctx.beginPath();
    ctx.rect(0, 0, size.w, size.h);
    if (selection) ctx.rect(selection.x, selection.y, selection.w, selection.h);
    ctx.fill("evenodd");

    if (selection) {
      ctx.strokeStyle = "#ff6369";
      ctx.lineWidth = 1;
      ctx.strokeRect(selection.x + 0.5, selection.y + 0.5, selection.w - 1, selection.h - 1);
      if (showHandles) {
        ctx.fillStyle = "#ffffff";
        for (const h of handles) {
          const c = handlePoint(selection, h);
          ctx.fillRect(c.x - 3.5, c.y - 3.5, 7, 7);
          ctx.strokeRect(c.x - 3.5, c.y - 3.5, 7, 7);
        }
      }
    }
  }

  const hoverWindow = $derived(
    tool === "select" && !selection && pointer && info?.windows.length && (!interaction || clickPending)
      ? windowAt(info.windows, toImage(pointer), scale, size)
      : null,
  );

  const showHandles = $derived(tool === "select" && (!interaction || interaction.kind === "area"));

  $effect(() => {
    void [selection, size, annotations, draft, selectedId, showHandles, hoverWindow];
    draw();
  });

  $effect(() => {
    if (textEdit && textEl && document.activeElement !== textEl) textEl.focus();
  });

  // ---- history -------------------------------------------------------------------------------

  function record() {
    history.record($state.snapshot(annotations) as Annotation[]);
    historyVersion++;
  }

  function undo() {
    const prev = history.undo($state.snapshot(annotations) as Annotation[]);
    if (prev) {
      annotations = prev;
      selectedId = null;
      historyVersion++;
    }
  }

  function redo() {
    const next = history.redo($state.snapshot(annotations) as Annotation[]);
    if (next) {
      annotations = next;
      selectedId = null;
      historyVersion++;
    }
  }

  const canUndo = $derived((void historyVersion, history.canUndo));
  const canRedo = $derived((void historyVersion, history.canRedo));

  function add(annotation: Annotation) {
    record();
    annotations = [...annotations, annotation];
  }

  function deleteSelected() {
    if (!selectedId) return;
    record();
    annotations = annotations.filter((a) => a.id !== selectedId);
    selectedId = null;
  }

  // ---- annotation helpers --------------------------------------------------------------------

  const currentStyle = (): Annotation["style"] => ({
    stroke: $state.snapshot(color),
    width: width * scale,
    fill: null,
    opacity: 1,
  });

  const newAnnotation = (shape: Annotation["shape"]): Annotation => ({
    id: `a${++nextId}`,
    frameStart: 0,
    frameEnd: 0,
    shape,
    style: currentStyle(),
  });

  function annotationAt(p: Point): Annotation | undefined {
    const tolerance = 6 * scale;
    return annotations.findLast((a) => hitTest(a.shape, p, tolerance + a.style.width / 2, a.style.width));
  }

  function commitText() {
    if (!textEdit) return;
    const value = (textEl?.innerText ?? textEdit.value).replace(/\n$/, "");
    const { at, style, fontSize } = textEdit;
    textEdit = null;
    const annotation: Annotation = {
      id: `a${++nextId}`,
      frameStart: 0,
      frameEnd: 0,
      shape: { type: "text", at, text: value, fontSize, background: null },
      style,
    };
    if (!isTrivial(annotation.shape)) add(annotation);
  }

  function startText(at: Point) {
    commitText();
    textEdit = { at, value: "", style: currentStyle(), fontSize: fontSizeFor(width) * scale };
  }

  function setTool(next: Tool) {
    commitText();
    tool = next;
    if (next !== "select") selectedId = null;
  }

  function setColor(next: Rgba) {
    color = next;
    restyleSelected({ stroke: next });
  }

  function setWidth(next: number) {
    width = next;
    restyleSelected({ width: next * scale });
  }

  /** Changing color/width while an annotation is selected applies it to that annotation. */
  function restyleSelected(change: Partial<Annotation["style"]>) {
    if (!selectedId) return;
    record();
    annotations = annotations.map((a) =>
      a.id === selectedId ? { ...a, style: { ...a.style, ...change } } : a,
    );
  }

  // ---- pointer -------------------------------------------------------------------------------

  const point = (e: PointerEvent | MouseEvent): Point => ({ x: e.clientX, y: e.clientY });

  function onpointerdown(e: PointerEvent) {
    if (busy) return;
    if (e.button === 2) return void cancelCapture();
    if (e.button !== 0) return;
    if (textEdit) {
      commitText();
      return;
    }
    const c = point(e);
    const p = toImage(c);
    canvas.setPointerCapture(e.pointerId);

    if (tool === "select") {
      const hit = annotationAt(p);
      if (hit) {
        selectedId = hit.id;
        interaction = { kind: "move", id: hit.id, last: p, recorded: false };
        return;
      }
      selectedId = null;
      clickedWindow = selection ? null : hoverWindow;
      clickPending = clickedWindow !== null;
      interaction = { kind: "area", drag: beginDrag(selection, c) };
    } else if (tool === "text") {
      startText(p);
    } else if (tool === "counter") {
      add(newAnnotation({ type: "counter", at: p, value: nextCounterValue(annotations) }));
    } else if (tool === "click") {
      add(newAnnotation({ type: "click", at: p }));
    } else if (isDragTool(tool)) {
      interaction = { kind: "draw", start: p, points: [p] };
    }
  }

  function onpointermove(e: PointerEvent) {
    const c = point(e);
    pointer = c;
    if (!interaction) {
      hoverHandle = tool === "select" && selection ? handleAt(selection, c) : null;
      hovering = tool === "select" && !hoverHandle && !!annotationAt(toImage(c));
      return;
    }
    const p = toImage(c);
    switch (interaction.kind) {
      case "area":
        selection = updateDrag(interaction.drag, c, size);
        if (selection.w >= 4 || selection.h >= 4) clickPending = false;
        break;
      case "draw": {
        interaction.points.push(p);
        const shape = shapeFromDrag(tool, interaction.start, p, [...interaction.points], width * scale);
        draft = shape ? newAnnotation(shape) : null;
        break;
      }
      case "move": {
        const { id, last } = interaction;
        if (!interaction.recorded) {
          record();
          interaction.recorded = true;
        }
        const dx = p.x - last.x;
        const dy = p.y - last.y;
        annotations = annotations.map((a) =>
          a.id === id ? { ...a, shape: translateShape(a.shape, dx, dy) } : a,
        );
        interaction.last = p;
        break;
      }
    }
  }

  function onpointerup(e: PointerEvent) {
    if (!interaction) return;
    if (canvas.hasPointerCapture(e.pointerId)) canvas.releasePointerCapture(e.pointerId);
    if (interaction.kind === "area" && selection && (selection.w < 2 || selection.h < 2)) selection = null;
    // A click (no drag) on a highlighted window selects that window.
    if (interaction.kind === "area" && clickPending && clickedWindow) selection = clickedWindow;
    clickPending = false;
    clickedWindow = null;
    if (interaction.kind === "draw" && draft && !isTrivial(draft.shape, 3 * scale)) add(draft);
    const chose = interaction.kind === "area" && selection !== null;
    draft = null;
    interaction = null;
    // Instant GIF: choosing the area (drag, or click on a window) starts the recording.
    if (instantMode && chose) void startRecording();
  }

  function ondblclick(e: MouseEvent) {
    // Double-clicking a text annotation edits it; anywhere else copies the screenshot.
    const hit = tool === "select" ? annotationAt(toImage(point(e))) : undefined;
    if (hit?.shape.type === "text") {
      record();
      annotations = annotations.filter((a) => a.id !== hit.id);
      selectedId = null;
      textEdit = { at: hit.shape.at, value: hit.shape.text, style: hit.style, fontSize: hit.shape.fontSize };
      return;
    }
    if (tool === "select") copy();
  }

  // ---- actions -------------------------------------------------------------------------------

  /** Runs a final action (copy, save, pin) on the selection, or the whole screen if none. */
  async function finish(action: "copy" | "save" | "saveAs" | "pin") {
    if (!info || busy) return;
    commitText();
    busy = true;
    const target = toImagePixels(selection ?? { x: 0, y: 0, w: size.w, h: size.h }, scale, info);
    const list = $state.snapshot(annotations) as Annotation[];
    try {
      if (action === "copy") await copySelection(target, list);
      else if (action === "pin") await pinSelection(target, list, scale);
      else if ((await saveSelection(target, list, action === "saveAs")) === null) busy = false; // dialog cancelled
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  const copy = () => finish("copy");

  // ---- record mode (choosing the area of a recording) -------------------------------------------

  const recordMode = $derived(info?.mode === "record" || info?.mode === "instant");
  /** Instant GIF: the recording starts as soon as the area is chosen. */
  const instantMode = $derived(info?.mode === "instant");

  /** Records the selection, or the whole screen with `whole` (or when nothing is selected). */
  async function startRecording(whole = false) {
    if (!info || busy) return;
    busy = true;
    try {
      await recordSelection(whole || !selection ? null : toImagePixels(selection, scale, info));
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  async function pickWindow() {
    if (busy) return;
    busy = true;
    try {
      await recordWindow();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    const ctrl = e.ctrlKey || e.metaKey;
    if (textEdit) {
      // Keys go to the text field; only Enter (confirm) and Esc (discard) are handled here.
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        commitText();
      } else if (e.key === "Escape") {
        e.preventDefault();
        textEdit = null;
      }
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      if (!interaction) cancelCapture();
    } else if (recordMode) {
      if (e.key === "Enter") {
        e.preventDefault();
        startRecording();
      }
    } else if (ctrl && e.key.toLowerCase() === "s") {
      e.preventDefault();
      finish(e.shiftKey ? "saveAs" : "save");
    } else if (e.key === "Enter" || (ctrl && e.key.toLowerCase() === "c")) {
      e.preventDefault();
      copy();
    } else if (ctrl && e.key.toLowerCase() === "z") {
      e.preventDefault();
      if (e.shiftKey) redo();
      else undo();
    } else if (ctrl && e.key.toLowerCase() === "y") {
      e.preventDefault();
      redo();
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      deleteSelected();
    } else if (!ctrl && !e.altKey && selection) {
      const next = toolForKey(e.key.toLowerCase());
      if (next) setTool(next);
    }
  }

  function onresize() {
    size = { w: window.innerWidth, h: window.innerHeight };
    if (info) scale = info.width / size.w;
  }

  // ---- layout --------------------------------------------------------------------------------

  const toolbarStyle = $derived.by(() => {
    if (!selection) return "";
    const p = toolbarPosition(selection, toolbarSize, size);
    return `left: ${p.x}px; top: ${p.y}px;`;
  });

  // Label next to the selection: above it, or inside if there is no room.
  const labelStyle = $derived.by(() => {
    if (!selection) return "";
    const top = selection.y >= 28 ? selection.y - 26 : selection.y + 4;
    return `left: ${selection.x}px; top: ${top}px;`;
  });

  const textStyle = $derived.by(() => {
    if (!textEdit) return "";
    const fontSize = textEdit.fontSize / scale;
    const c = textEdit.style.stroke;
    return [
      `left: ${textEdit.at.x / scale}px`,
      `top: ${textEdit.at.y / scale}px`,
      `font: ${fontSize}px/${LINE_HEIGHT} ${FONT_FAMILY}`,
      `color: rgb(${c.r} ${c.g} ${c.b})`,
      `min-width: ${fontSize}px`,
    ].join("; ");
  });

  const resizeHandle = $derived(
    interaction?.kind === "area" && interaction.drag.kind === "resize"
      ? interaction.drag.handle
      : hoverHandle,
  );

  // The loupe helps while choosing the area: before there is one, and while drawing/resizing it.
  const showLoupe = $derived(
    tool === "select" &&
      pointer !== null &&
      pixels !== null &&
      (interaction
        ? interaction.kind === "area" && interaction.drag.kind !== "move"
        : !selection && !hovering),
  );

  const cursor = $derived(
    resizeHandle
      ? handleCursor[resizeHandle]
      : tool === "select"
        ? hovering
          ? "move"
          : "crosshair"
        : tool === "text"
          ? "text"
          : "crosshair",
  );
  const sameColor = (a: Rgba, b: Rgba) => a.r === b.r && a.g === b.g && a.b === b.b;
</script>

<!-- Track the cursor on the window, not only the canvas: some compositors deliver motion only to
     the surface, and the loupe must follow the mouse everywhere. -->
<svelte:window
  {onkeydown}
  {onresize}
  oncontextmenu={(e) => e.preventDefault()}
  onpointermove={(e) => (pointer = point(e))}
  onmousemove={(e) => (pointer = point(e))}
/>

<canvas
  bind:this={canvas}
  tabindex="-1"
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  {ondblclick}
  style:width="{size.w}px"
  style:height="{size.h}px"
  style:cursor
  onpointerleave={() => (pointer = null)}
></canvas>

{#if showLoupe && pointer && pixels && info}
  <Magnifier
    {pixels}
    imageWidth={info.width}
    imageHeight={info.height}
    at={toImage(pointer)}
    cursor={pointer}
    screen={size}
  />
{/if}

{#if textEdit}
  <div
    class="text-edit"
    bind:this={textEl}
    contenteditable="plaintext-only"
    role="textbox"
    tabindex="0"
    aria-multiline="true"
    aria-label={t("tool.text")}
    data-placeholder={t("overlay.textPlaceholder")}
    style={textStyle}
  >
    {textEdit.value}
  </div>
{/if}

{#if selection && imageSelection}
  <div class="size" style={labelStyle}>{imageSelection.width} × {imageSelection.height}</div>
  {#if !interaction && recordMode}
    <div
      class="toolbar"
      style={toolbarStyle}
      bind:clientWidth={toolbarSize.w}
      bind:clientHeight={toolbarSize.h}
    >
      <div class="row">
        <button
          class="primary"
          title={t("record.regionTitle")}
          onclick={() => startRecording()}
          disabled={busy}
        >
          <span class="rec-dot"></span>
          {t("record.region")}
          <kbd>Enter</kbd>
        </button>
        <button
          class="icon"
          title={t("overlay.cancelTitle")}
          aria-label={t("overlay.cancel")}
          onclick={() => cancelCapture()}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
      </div>
    </div>
  {:else if !interaction}
    <div
      class="toolbar"
      style={toolbarStyle}
      bind:clientWidth={toolbarSize.w}
      bind:clientHeight={toolbarSize.h}
    >
      <div class="row">
        {#each tools as item (item.id)}
          <button
            class="icon"
            class:active={tool === item.id}
            title="{t(`tool.${item.id}`)} ({item.key.toUpperCase()})"
            aria-label={t(`tool.${item.id}`)}
            onclick={() => setTool(item.id)}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d={item.icon} /></svg>
          </button>
        {/each}
        <span class="sep"></span>
        <button
          class="icon"
          title={t("overlay.undo")}
          aria-label={t("overlay.undo")}
          onclick={undo}
          disabled={!canUndo}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 14L4 9l5-5M4 9h11a5 5 0 0 1 0 10h-3" /></svg
          >
        </button>
        <button
          class="icon"
          title={t("overlay.redo")}
          aria-label={t("overlay.redo")}
          onclick={redo}
          disabled={!canRedo}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 14l5-5-5-5M20 9H9a5 5 0 0 0 0 10h3" /></svg
          >
        </button>
        {#if selectedId}
          <button
            class="icon"
            title={t("overlay.delete")}
            aria-label={t("overlay.delete")}
            onclick={deleteSelected}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true"
              ><path d="M5 7h14M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" /></svg
            >
          </button>
        {/if}
      </div>
      <div class="row">
        <div class="swatches" role="radiogroup" aria-label={t("overlay.color")}>
          {#each colors as c (toHex(c))}
            <button
              class="swatch"
              class:active={sameColor(c, color)}
              style:background={toHex(c)}
              role="radio"
              aria-checked={sameColor(c, color)}
              aria-label={toHex(c)}
              onclick={() => setColor(c)}
            ></button>
          {/each}
          <label class="swatch custom" title={t("overlay.customColor")} style:background={toHex(color)}>
            <input
              type="color"
              value={toHex(color)}
              oninput={(e) => setColor(fromHex(e.currentTarget.value))}
            />
          </label>
        </div>
        <span class="sep"></span>
        <div class="widths" role="radiogroup" aria-label={t("overlay.width")}>
          {#each widths as w (w)}
            <button
              class="icon"
              class:active={width === w}
              role="radio"
              aria-checked={width === w}
              aria-label="{t('overlay.width')} {w}"
              onclick={() => setWidth(w)}
            >
              <span class="dot" style:width="{w + 2}px" style:height="{w + 2}px"></span>
            </button>
          {/each}
        </div>
        <span class="spacer"></span>
        <button
          class="icon"
          title={t("overlay.save", { dir: saveDirectory })}
          aria-label={t("overlay.save", { dir: saveDirectory })}
          onclick={() => finish("save")}
          disabled={busy}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M5 4h11l3 3v12a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zM8 4v5h7V4M8 20v-6h8v6" />
          </svg>
        </button>
        <button
          class="icon"
          title={t("overlay.saveAs")}
          aria-label={t("overlay.saveAs")}
          onclick={() => finish("saveAs")}
          disabled={busy}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M11 20H6a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h10l3 3v3M8 4v5h7V4M13 21l.5-3 5-5 2.5 2.5-5 5z" />
          </svg>
        </button>
        <button
          class="icon"
          title={t("overlay.pin")}
          aria-label={t("overlay.pin")}
          onclick={() => finish("pin")}
          disabled={busy}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M15 4l5 5-3 1-3 3 1 5-2 1-4-4-5 5M9 15l-4-4 1-2 5 1 3-3 1-3" />
          </svg>
        </button>
        <button class="primary" title={t("overlay.copyTitle")} onclick={copy} disabled={busy}>
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M10 8h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-8a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z" />
            <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
          </svg>
          {t("overlay.copy")}
          <kbd>Enter</kbd>
        </button>
        <button
          class="icon"
          title={t("overlay.cancelTitle")}
          aria-label={t("overlay.cancel")}
          onclick={() => cancelCapture()}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
      </div>
    </div>
  {/if}
{:else if !error && recordMode && !interaction}
  <div class="record-panel">
    <span
      >{instantMode
        ? t("record.instantHint")
        : t(info?.windows.length ? "record.hintWithWindows" : "record.hint")}</span
    >
    <div class="row">
      <button class="primary" onclick={() => startRecording(true)} disabled={busy}>
        <span class="rec-dot"></span>
        {t("record.fullscreen")}
      </button>
      {#if info?.windowPicker}
        <button class="secondary" title={t("record.windowTitle")} onclick={pickWindow} disabled={busy}>
          {t("record.window")}
        </button>
      {/if}
      <button
        class="icon"
        title={t("overlay.cancelTitle")}
        aria-label={t("overlay.cancel")}
        onclick={() => cancelCapture()}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
      </button>
    </div>
  </div>
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
  .record-panel {
    position: fixed;
    top: 24px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border-radius: 10px;
    background: rgb(20 20 24 / 94%);
    box-shadow: 0 4px 16px rgb(0 0 0 / 40%);
    color: #fff;
    font-size: 13px;
  }
  button.secondary {
    height: 32px;
    padding: 0 10px;
    background: rgb(255 255 255 / 12%);
  }
  .rec-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 0 0 2px rgb(255 255 255 / 35%);
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
  .text-edit {
    position: fixed;
    margin: 0;
    padding: 0;
    white-space: pre;
    outline: 1px dashed rgb(255 255 255 / 80%);
    outline-offset: 2px;
    caret-color: currentColor;
  }
  .text-edit:empty::before {
    content: attr(data-placeholder);
    font-size: 13px;
    color: rgb(255 255 255 / 70%);
    background: rgb(20 20 24 / 85%);
    padding: 2px 6px;
    border-radius: 4px;
  }
  .toolbar {
    position: fixed;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px;
    border-radius: 10px;
    background: rgb(20 20 24 / 94%);
    box-shadow: 0 4px 16px rgb(0 0 0 / 40%);
    color: #fff;
    font-size: 13px;
  }
  .row,
  .swatches,
  .widths {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .sep {
    width: 1px;
    align-self: stretch;
    margin: 4px 4px;
    background: rgb(255 255 255 / 15%);
  }
  .spacer {
    flex: 1;
    min-width: 12px;
  }
  button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.35;
    cursor: default;
  }
  button.icon {
    width: 32px;
    height: 32px;
  }
  button:hover:not(:disabled) {
    background: rgb(255 255 255 / 12%);
  }
  button.active {
    background: rgb(255 255 255 / 20%);
  }
  button.primary {
    padding: 0 10px;
    height: 32px;
    background: #e5484d;
  }
  button.primary:hover:not(:disabled) {
    background: #f15a5f;
  }
  svg {
    width: 18px;
    height: 18px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .swatch {
    width: 20px;
    height: 20px;
    margin: 0 2px;
    border-radius: 50%;
    border: 2px solid rgb(255 255 255 / 25%);
    padding: 0;
  }
  .swatch.active {
    border-color: #fff;
    box-shadow: 0 0 0 2px rgb(255 255 255 / 35%);
  }
  .swatch.custom {
    position: relative;
    overflow: hidden;
    cursor: pointer;
    background-image: conic-gradient(red, yellow, lime, cyan, blue, magenta, red) !important;
  }
  .swatch.custom input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }
  .dot {
    display: block;
    border-radius: 50%;
    background: currentColor;
  }
  kbd {
    padding: 0 5px;
    border-radius: 4px;
    background: rgb(255 255 255 / 15%);
    font:
      11px/18px ui-monospace,
      monospace;
  }
</style>
