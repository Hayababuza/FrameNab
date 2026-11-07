<script lang="ts">
  /**
   * Editor for recordings: preview with playback, a filmstrip of all frames, deleting frames,
   * annotations over frame ranges, undo/redo, and saving the result into the save folder.
   */
  import { listen } from "@tauri-apps/api/event";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import type { Annotation } from "$lib/bindings/Annotation";
  import type { Rgba } from "$lib/bindings/Rgba";
  import { colors, type Tool, toolForKey, widths } from "$lib/annotations/tools";
  import AnnotationLayer from "$lib/components/AnnotationLayer.svelte";
  import AnnotationTools from "$lib/components/AnnotationTools.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import {
    editorCancelExport,
    editorDelete,
    editorExport,
    editorSetAnnotations,
    editorUndo,
    editorView,
    previewUrl,
    runAction,
    thumbUrl,
    type EditorView,
  } from "$lib/ipc";
  import { getSetting, type RecordingFormat } from "$lib/stores/settings";

  const THUMB_HEIGHT = 90;
  const formatNames: Record<RecordingFormat, string> = { gif: "GIF", mp4: "MP4", webm: "WebM" };

  let view = $state<EditorView | null>(null);
  let current = $state(0);
  let selected = $state<Set<number>>(new Set());
  let anchor = 0;
  let playing = $state(false);
  let playTimer: ReturnType<typeof setTimeout> | undefined;
  let format = $state<RecordingFormat>("gif");
  let progress = $state<[number, number] | null>(null);
  let saved = $state<string | null>(null);
  let error = $state<string | null>(null);
  let strip: HTMLDivElement | undefined = $state();
  /** Filmstrip scroll position and visible width, for rendering only the visible frames. */
  let scrollLeft = $state(0);
  let stripWidth = $state(0);
  /** Selecting by dragging over the filmstrip. */
  let drag: { anchor: number; base: Set<number> | null; x: number } | null = null;
  let autoScroll: ReturnType<typeof setInterval> | undefined;

  // Annotations
  let tool = $state<Tool>("select");
  let color = $state<Rgba>(colors[0]);
  let strokeWidth = $state(widths[1]);
  let selectedId = $state<string | null>(null);

  const frames = $derived(view?.project.frames ?? []);
  const duration = $derived(frames.reduce((sum, f) => sum + f.delayMs, 0));
  const thumbWidth = $derived(
    view
      ? Math.max(1, Math.round((view.project.width * THUMB_HEIGHT) / Math.max(1, view.project.height)))
      : 160,
  );
  const annotations = $derived(view?.project.annotations ?? []);
  const selectedAnnotation = $derived(annotations.find((a) => a.id === selectedId) ?? null);
  /** Which frames new annotations cover: the whole recording (default) or the selected frames. */
  let scope = $state<"all" | "selection">("all");
  /** The selected frames, or the current frame if none are selected. */
  const selectionRange = $derived.by((): [number, number] => {
    if (!selected.size) return [current, current];
    const list = [...selected];
    return [Math.min(...list), Math.max(...list)];
  });
  const allRange = $derived<[number, number]>([0, Math.max(0, frames.length - 1)]);
  const range = $derived(scope === "all" ? allRange : selectionRange);

  async function setAnnotations(list: Annotation[]) {
    view = await editorSetAnnotations(list);
    if (selectedId && !list.some((a) => a.id === selectedId)) selectedId = null;
  }

  /** Changing color or width while an annotation is selected restyles it. */
  function restyle(change: { color?: Rgba; width?: number }) {
    if (!selectedAnnotation) return;
    setAnnotations(
      annotations.map((a) =>
        a.id === selectedId
          ? {
              ...a,
              style: {
                ...a.style,
                ...(change.color ? { stroke: change.color } : {}),
                ...(change.width ? { width: change.width } : {}),
              },
            }
          : a,
      ),
    );
  }

  function deleteAnnotation() {
    if (selectedId) setAnnotations(annotations.filter((a) => a.id !== selectedId));
  }

  /** Puts the selected annotation on the selected frames. */
  /** Puts the selected annotation on all frames, or on the selected frames. */
  function applyRange(to: "all" | "selection") {
    if (!selectedId) return;
    const [frameStart, frameEnd] = to === "all" ? allRange : selectionRange;
    setAnnotations(annotations.map((a) => (a.id === selectedId ? { ...a, frameStart, frameEnd } : a)));
  }

  /** Frames that have at least one annotation (for the filmstrip markers). */
  const annotated = $derived.by(() => {
    const covered: boolean[] = [];
    for (const a of annotations) for (let i = a.frameStart; i <= a.frameEnd; i++) covered[i] = true;
    return covered;
  });

  /** Frames to delete: the selection, or the current frame if nothing is selected. */
  const toDelete = $derived(
    selected.size ? [...selected].sort((a, b) => a - b) : frames.length ? [current] : [],
  );

  async function load() {
    view = await editorView();
    current = Math.min(current, Math.max(0, frames.length - 1));
  }

  onMount(() => {
    load();
    getSetting("recordingFormat").then((f) => (format = f));
    const unlisteners = [
      listen("editor-reload", () => {
        stop();
        selected = new Set();
        current = 0;
        saved = null;
        load();
      }),
      listen<[number, number]>("export-progress", (e) => {
        if (progress) progress = e.payload;
      }),
      listen("settings-changed", () => getSetting("recordingFormat").then((f) => (format = f))),
    ];
    return () => {
      stop();
      unlisteners.forEach((u) => void u.then((fn) => fn()));
    };
  });

  // ---- selection -----------------------------------------------------------------------------

  function select(i: number, e: MouseEvent | KeyboardEvent) {
    stop();
    if (e.shiftKey) {
      const [a, b] = [Math.min(anchor, i), Math.max(anchor, i)];
      selected = new Set(Array.from({ length: b - a + 1 }, (_, k) => a + k));
    } else if (e.ctrlKey || e.metaKey) {
      // A new Set (not mutated in place) so the change is picked up.
      selected = selected.has(i) ? new Set([...selected].filter((x) => x !== i)) : new Set([...selected, i]);
      anchor = i;
    } else {
      selected = new Set([i]);
      anchor = i;
    }
    current = i;
  }

  function goTo(i: number) {
    current = Math.max(0, Math.min(frames.length - 1, i));
  }

  // ---- filmstrip (virtualized) -----------------------------------------------------------------

  const GAP = 6;
  const PAD = 12;
  const stride = $derived(thumbWidth + GAP);
  /** Only frames in (or near) the visible part are rendered. */
  const visible = $derived.by(() => {
    const first = Math.max(0, Math.floor((scrollLeft - PAD) / stride) - 4);
    const last = Math.min(frames.length, Math.ceil((scrollLeft + stripWidth) / stride) + 4);
    return frames.slice(first, last).map((frame, k) => ({ frame, i: first + k }));
  });

  /** Keeps the current frame in view. */
  $effect(() => {
    void current;
    if (!strip) return;
    const left = PAD + current * stride;
    if (left < strip.scrollLeft) strip.scrollLeft = left - PAD;
    else if (left + thumbWidth > strip.scrollLeft + strip.clientWidth)
      strip.scrollLeft = left + thumbWidth + PAD - strip.clientWidth;
  });

  /** The mouse wheel scrolls the filmstrip sideways. */
  function onwheel(e: WheelEvent) {
    if (!strip || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
    e.preventDefault();
    strip.scrollLeft += e.deltaY;
  }

  /** Index of the frame under screen x (clamped). */
  function indexAt(x: number) {
    if (!strip) return current;
    const offset = x - strip.getBoundingClientRect().left + strip.scrollLeft - PAD;
    return Math.max(0, Math.min(frames.length - 1, Math.floor(offset / stride)));
  }

  function onframedown(i: number, e: MouseEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    select(i, e);
    drag = {
      anchor: e.shiftKey ? anchor : i,
      base: e.ctrlKey || e.metaKey ? new Set(selected) : null,
      x: e.clientX,
    };
    // Near the edges, keep scrolling while dragging.
    autoScroll = setInterval(() => {
      if (!drag || !strip) return;
      const r = strip.getBoundingClientRect();
      const step = drag.x < r.left + 40 ? -16 : drag.x > r.right - 40 ? 16 : 0;
      if (step) {
        strip.scrollLeft += step;
        extendDrag();
      }
    }, 16);
  }

  function extendDrag() {
    if (!drag) return;
    const i = indexAt(drag.x);
    const [a, b] = [Math.min(drag.anchor, i), Math.max(drag.anchor, i)];
    const range = Array.from({ length: b - a + 1 }, (_, k) => a + k);
    selected = new Set(drag.base ? [...drag.base, ...range] : range);
    current = i;
  }

  function ondragmove(e: MouseEvent) {
    if (!drag) return;
    drag.x = e.clientX;
    extendDrag();
  }

  function enddrag() {
    drag = null;
    clearInterval(autoScroll);
  }

  // ---- edits ---------------------------------------------------------------------------------

  async function deleteFrames() {
    if (!toDelete.length || toDelete.length >= frames.length) return;
    stop();
    const first = toDelete[0];
    view = await editorDelete(toDelete);
    selected = new Set();
    goTo(first);
  }

  async function undo(redo = false) {
    stop();
    view = await editorUndo(redo);
    selected = new Set();
    goTo(current);
  }

  // ---- playback ------------------------------------------------------------------------------

  function stop() {
    playing = false;
    clearTimeout(playTimer);
  }

  function play() {
    if (playing) return stop();
    if (frames.length < 2) return;
    playing = true;
    const step = () => {
      if (!playing) return;
      const delay = frames[current]?.delayMs ?? 100;
      // Preload the next frame so playback does not flicker.
      const next = (current + 1) % frames.length;
      new Image().src = previewUrl(frames[next].id);
      playTimer = setTimeout(() => {
        current = next;
        step();
      }, delay);
    };
    step();
  }

  // ---- saving --------------------------------------------------------------------------------

  async function save(ask = false) {
    if (progress || !frames.length) return;
    stop();
    error = null;
    saved = null;
    progress = [0, frames.length];
    try {
      const path = await editorExport(ask);
      if (path) saved = path;
    } catch (e) {
      const message = String(e);
      if (!message.toLowerCase().includes("cancel")) error = message;
    } finally {
      progress = null;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if ((e.target as HTMLElement).closest("input, select, textarea")) return;
    const ctrl = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    const actions: [boolean, () => void][] = [
      [ctrl && key === "z", () => undo(e.shiftKey)],
      [ctrl && key === "y", () => undo(true)],
      [ctrl && key === "s", () => save(e.shiftKey)],
      [ctrl && key === "a", () => (selected = new Set(frames.map((_, i) => i)))],
      [e.key === "Delete" || e.key === "Backspace", () => (selectedId ? deleteAnnotation() : deleteFrames())],
      [e.key === " ", () => play()],
      [e.key === "ArrowLeft", () => (stop(), goTo(current - 1))],
      [e.key === "ArrowRight", () => (stop(), goTo(current + 1))],
      [e.key === "Home", () => (stop(), goTo(0))],
      [e.key === "End", () => (stop(), goTo(frames.length - 1))],
      [e.key === "Escape", () => (selectedId ? (selectedId = null) : (selected = new Set()))],
      [!ctrl && !e.altKey && !!toolForKey(key), () => (tool = toolForKey(key)!)],
    ];
    const action = actions.find(([matches]) => matches);
    if (action) {
      e.preventDefault();
      action[1]();
    }
  }

  const time = (ms: number) => {
    const s = ms / 1000;
    return `${Math.floor(s / 60)}:${(s % 60).toFixed(1).padStart(4, "0")}`;
  };
  /** Start time of frame `i`. */
  const startOf = (i: number) => frames.slice(0, i).reduce((sum, f) => sum + f.delayMs, 0);
</script>

<svelte:window {onkeydown} onmousemove={ondragmove} onmouseup={enddrag} />

{#if !view}
  <p class="empty">{t("editor.empty")}</p>
{:else}
  <div class="editor">
    <header>
      <button
        class="icon"
        title={t("editor.undo")}
        aria-label={t("editor.undo")}
        onclick={() => undo()}
        disabled={!view.canUndo}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 14L4 9l5-5M4 9h11a5 5 0 0 1 0 10h-3" /></svg>
      </button>
      <button
        class="icon"
        title={t("editor.redo")}
        aria-label={t("editor.redo")}
        onclick={() => undo(true)}
        disabled={!view.canRedo}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 14l5-5-5-5M20 9H9a5 5 0 0 0 0 10h3" /></svg>
      </button>
      <button
        title={t("editor.delete")}
        onclick={deleteFrames}
        disabled={!toDelete.length || toDelete.length >= frames.length}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M5 7h14M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" /></svg
        >
        {t("editor.deleteCount", { n: toDelete.length })}
      </button>
      <span class="info">
        {t("editor.info", {
          frames: frames.length,
          duration: time(duration),
          w: view.project.width,
          h: view.project.height,
        })}
      </span>
      <span class="spacer"></span>
      <button
        title={t("editor.newRecording")}
        onclick={() => (stop(), runAction("record"))}
        disabled={!!progress}
      >
        <span class="rec-dot"></span>
        {t("editor.newRecording")}
      </button>
      {#if progress}
        <span class="progress">
          <span class="bar"
            ><span style:width="{(progress[0] / Math.max(1, progress[1])) * 100}%"></span></span
          >
          {t("editor.saving", { done: progress[0], total: progress[1] })}
        </span>
        <button onclick={() => editorCancelExport()}>{t("editor.cancel")}</button>
      {:else}
        <button title={t("editor.saveAs")} onclick={() => save(true)}>
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M11 20H6a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h10l3 3v3M8 4v5h7V4M13 21l.5-3 5-5 2.5 2.5-5 5z" />
          </svg>
          {t("editor.saveAs").replace(/ \(.*\)$/, "")}
        </button>
        <button
          class="primary"
          title={t("editor.save", { format: formatNames[format] })}
          onclick={() => save()}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M5 4h11l3 3v12a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zM8 4v5h7V4M8 20v-6h8v6" />
          </svg>
          {t("editor.save", { format: formatNames[format] }).replace(/ \(.*\)$/, "")}
        </button>
      {/if}
    </header>

    <div class="annotate">
      <AnnotationTools bind:tool bind:color bind:width={strokeWidth} onstyle={restyle} disabled={playing} />
      <span class="spacer"></span>
      {#if selectedAnnotation}
        <span class="range">
          {t("editor.annotationRange", {
            a: selectedAnnotation.frameStart + 1,
            b: selectedAnnotation.frameEnd + 1,
          })}
        </span>
        <button onclick={() => applyRange("all")}>{t("editor.applyAll")}</button>
        <button onclick={() => applyRange("selection")} title={t("editor.applyRangeTitle")}>
          {t("editor.applyRange")}
        </button>
        <button onclick={deleteAnnotation}>{t("editor.deleteAnnotation")}</button>
      {:else}
        <span class="range">{t("editor.scope")}</span>
        <div class="segmented" role="radiogroup" aria-label={t("editor.scope")}>
          <button
            role="radio"
            aria-checked={scope === "all"}
            class:active={scope === "all"}
            onclick={() => (scope = "all")}
          >
            {t("editor.scopeAll")}
          </button>
          <button
            role="radio"
            aria-checked={scope === "selection"}
            class:active={scope === "selection"}
            onclick={() => (scope = "selection")}
            title={t("editor.scopeSelectionTitle")}
          >
            {t("editor.scopeSelection", { a: selectionRange[0] + 1, b: selectionRange[1] + 1 })}
          </button>
        </div>
      {/if}
    </div>

    {#if saved}
      <div class="notice">
        <span>{t("editor.saved", { path: saved })}</span>
        <button onclick={() => saved && revealItemInDir(saved)}>{t("editor.openFolder")}</button>
      </div>
    {/if}
    {#if error}
      <div class="notice error">{error}</div>
    {/if}

    <main class="preview">
      {#if frames[current]}
        <AnnotationLayer
          src={previewUrl(frames[current].id)}
          width={view.project.width}
          height={view.project.height}
          {annotations}
          frame={current}
          {range}
          {tool}
          {color}
          {strokeWidth}
          bind:selectedId
          readonly={playing}
          onchange={setAnnotations}
        />
      {/if}
    </main>

    <div class="transport">
      <button
        class="icon"
        title={t("editor.prev")}
        aria-label={t("editor.prev")}
        onclick={() => (stop(), goTo(current - 1))}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 6l-6 6 6 6" /></svg>
      </button>
      <button
        class="icon play"
        title={playing ? t("editor.pause") : t("editor.play")}
        aria-label={playing ? t("editor.pause") : t("editor.play")}
        onclick={play}
      >
        {#if playing}
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5v14M16 5v14" /></svg>
        {:else}
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 5l12 7-12 7z" /></svg>
        {/if}
      </button>
      <button
        class="icon"
        title={t("editor.next")}
        aria-label={t("editor.next")}
        onclick={() => (stop(), goTo(current + 1))}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
      </button>
      <span class="position">
        {t("editor.frame", { i: current + 1, n: frames.length })} · {time(startOf(current))} / {time(
          duration,
        )}
      </span>
      <span class="hint">{t("editor.hint")}</span>
    </div>

    <div
      class="strip"
      bind:this={strip}
      bind:clientWidth={stripWidth}
      onscroll={() => strip && (scrollLeft = strip.scrollLeft)}
      {onwheel}
    >
      <div class="track" style:width="{PAD * 2 + frames.length * stride - GAP}px">
        {#each visible as { frame, i } (frame.id)}
          <button
            class="frame"
            class:current={i === current}
            class:selected={selected.has(i)}
            class:in-range={!!selectedAnnotation &&
              i >= selectedAnnotation.frameStart &&
              i <= selectedAnnotation.frameEnd}
            onmousedown={(e) => onframedown(i, e)}
            style:left="{PAD + i * stride}px"
            style:width="{thumbWidth}px"
          >
            <img
              src={thumbUrl(frame.id)}
              alt=""
              loading="lazy"
              draggable="false"
              style:height="{THUMB_HEIGHT}px"
            />
            <span class="meta"
              ><b
                >{i + 1}{#if annotated[i]}<i class="mark" title={t("editor.hasAnnotation")}></i>{/if}</b
              >{frame.delayMs} ms</span
            >
          </button>
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .empty {
    padding: 32px;
    color: var(--text-muted);
  }
  .editor {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .editor > :global(*) {
    flex: none;
  }
  header,
  .transport {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }
  .transport {
    border-top: 1px solid var(--border);
    border-bottom: none;
  }
  .info,
  .position {
    margin-left: 8px;
    color: var(--text-muted);
    font-size: 0.9rem;
    font-variant-numeric: tabular-nums;
  }
  .hint {
    margin-left: auto;
    color: var(--text-muted);
    font-size: 0.8rem;
  }
  .rec-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--accent);
  }
  .spacer {
    flex: 1;
  }
  button {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: var(--surface-hover);
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  button.icon {
    width: 32px;
    padding: 0;
    justify-content: center;
  }
  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
  }
  button.primary:hover:not(:disabled) {
    background: var(--accent);
    filter: brightness(1.1);
  }
  svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .play svg {
    fill: currentColor;
  }
  .notice {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    background: var(--info-bg);
    color: var(--info-text);
    font-size: 0.9rem;
  }
  .notice.error {
    background: rgb(160 30 30 / 15%);
    color: #e5484d;
  }
  .progress {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.9rem;
    font-variant-numeric: tabular-nums;
  }
  .progress .bar {
    width: 160px;
    height: 6px;
    border-radius: 3px;
    background: var(--surface-hover);
    overflow: hidden;
  }
  .progress .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.15s;
  }
  .preview {
    flex: 1 1 auto;
    display: grid;
    place-items: center;
    min-height: 0;
    overflow: hidden;
    padding: 8px;
    background: var(--bg);
  }
  /* Fill the available space (also enlarging small recordings), keeping the aspect ratio. */
  .annotate {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 12px;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }
  .range {
    color: var(--text-muted);
    font-size: 0.85rem;
  }
  .annotate button {
    white-space: nowrap;
  }
  .segmented {
    display: flex;
  }
  .segmented button {
    border-radius: 0;
  }
  .segmented button:first-child {
    border-radius: 6px 0 0 6px;
  }
  .segmented button:last-child {
    border-radius: 0 6px 6px 0;
    border-left: none;
  }
  .segmented button.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  .frame.in-range {
    outline: 2px dashed var(--accent);
    outline-offset: 1px;
  }
  .mark {
    display: inline-block;
    width: 6px;
    height: 6px;
    margin-left: 4px;
    border-radius: 50%;
    background: var(--accent);
    vertical-align: middle;
  }
  .strip {
    overflow-x: auto;
    overflow-y: hidden;
    background: var(--surface);
    border-top: 1px solid var(--border);
  }
  .track {
    position: relative;
    height: 128px;
  }
  .frame {
    position: absolute;
    top: 10px;
    user-select: none;
    flex-direction: column;
    align-items: stretch;
    gap: 2px;
    height: auto;
    padding: 3px;
    border: 2px solid transparent;
    background: var(--bg);
  }
  .frame img {
    display: block;
    width: 100%;
    object-fit: cover;
    border-radius: 3px;
    background: var(--surface-hover);
  }
  .frame .meta {
    display: flex;
    justify-content: space-between;
    padding: 0 2px;
    font-size: 0.72rem;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }
  .frame.selected {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 18%, var(--bg));
  }
  .frame.current {
    outline: 2px solid var(--text);
    outline-offset: 1px;
  }
</style>
