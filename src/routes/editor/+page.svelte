<script lang="ts">
  /**
   * Editor for recordings (first version): preview with playback, a filmstrip of all frames,
   * deleting frames with undo/redo, and saving the result into the save folder.
   */
  import { listen } from "@tauri-apps/api/event";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { onMount, tick } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import {
    editorCancelExport,
    editorDelete,
    editorExport,
    editorUndo,
    editorView,
    previewUrl,
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

  const frames = $derived(view?.project.frames ?? []);
  const duration = $derived(frames.reduce((sum, f) => sum + f.delayMs, 0));
  const thumbWidth = $derived(
    view
      ? Math.max(1, Math.round((view.project.width * THUMB_HEIGHT) / Math.max(1, view.project.height)))
      : 160,
  );
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

  $effect(() => {
    void current;
    tick().then(() =>
      strip?.querySelector(".frame.current")?.scrollIntoView({ block: "nearest", inline: "nearest" }),
    );
  });

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

  async function save() {
    if (progress || !frames.length) return;
    stop();
    error = null;
    saved = null;
    progress = [0, frames.length];
    try {
      saved = await editorExport();
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
      [ctrl && key === "s", () => save()],
      [ctrl && key === "a", () => (selected = new Set(frames.map((_, i) => i)))],
      [e.key === "Delete" || e.key === "Backspace", () => deleteFrames()],
      [e.key === " ", () => play()],
      [e.key === "ArrowLeft", () => (stop(), goTo(current - 1))],
      [e.key === "ArrowRight", () => (stop(), goTo(current + 1))],
      [e.key === "Home", () => (stop(), goTo(0))],
      [e.key === "End", () => (stop(), goTo(frames.length - 1))],
      [e.key === "Escape", () => (selected = new Set())],
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

<svelte:window {onkeydown} />

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
      {#if progress}
        <span class="progress">
          <span class="bar"
            ><span style:width="{(progress[0] / Math.max(1, progress[1])) * 100}%"></span></span
          >
          {t("editor.saving", { done: progress[0], total: progress[1] })}
        </span>
        <button onclick={() => editorCancelExport()}>{t("editor.cancel")}</button>
      {:else}
        <button class="primary" title={t("editor.save", { format: formatNames[format] })} onclick={save}>
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M5 4h11l3 3v12a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zM8 4v5h7V4M8 20v-6h8v6" />
          </svg>
          {t("editor.save", { format: formatNames[format] }).replace(/ \(.*\)$/, "")}
        </button>
      {/if}
    </header>

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
        <img src={previewUrl(frames[current].id)} alt="" draggable="false" />
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

    <div class="strip" bind:this={strip}>
      {#each frames as frame, i (frame.id)}
        <button
          class="frame"
          class:current={i === current}
          class:selected={selected.has(i)}
          onclick={(e) => select(i, e)}
          style:width="{thumbWidth}px"
        >
          <img
            src={thumbUrl(frame.id)}
            alt=""
            loading="lazy"
            draggable="false"
            style:height="{THUMB_HEIGHT}px"
          />
          <span class="meta"><b>{i + 1}</b>{frame.delayMs} ms</span>
        </button>
      {/each}
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
    padding: 16px;
    background: repeating-conic-gradient(var(--surface-hover) 0 25%, var(--bg) 0 50%) 0 0 / 20px 20px;
  }
  .preview img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    box-shadow: var(--shadow);
  }
  .strip {
    display: flex;
    gap: 6px;
    padding: 10px 12px 12px;
    overflow-x: auto;
    background: var(--surface);
    border-top: 1px solid var(--border);
  }
  .frame {
    flex: none;
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
