<script lang="ts">
  /**
   * Export dialog of the editor: format, quality, lossy compression, repetitions and size, with
   * an estimate of the file size. `onsave` gets the choice; saving itself is up to the caller.
   */
  import { onMount } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { editorEstimate, recordingFormats, type ExportFormat } from "$lib/ipc";
  import type { ExportOptions } from "$lib/stores/settings";

  interface Props {
    format: ExportFormat;
    options: ExportOptions;
    width: number;
    height: number;
    /** `ask`: pick the location in a "Save as" dialog; otherwise into the save folder. */
    onsave: (format: ExportFormat, options: ExportOptions, ask: boolean) => void;
    oncancel: () => void;
  }

  let { format: initialFormat, options: initialOptions, width, height, onsave, oncancel }: Props = $props();

  // Start from the given choice, then edit locally.
  // svelte-ignore state_referenced_locally
  let format = $state<ExportFormat>(initialFormat);
  // svelte-ignore state_referenced_locally
  let options = $state<ExportOptions>({ ...initialOptions });
  let available = $state<ExportFormat[]>(["gif", "apng", "webp", "pngSequence"]);
  let estimate = $state<number | null>(null);
  let estimating = $state(false);
  let dialog: HTMLDivElement;

  const ffmpeg = $derived(available.includes("mp4"));
  const labels: Record<ExportFormat, string> = {
    gif: "GIF",
    apng: "APNG",
    webp: "WebP",
    pngSequence: "PNG (ZIP)",
    mp4: "MP4",
    webm: "WebM",
  };
  const order: ExportFormat[] = ["gif", "apng", "webp", "pngSequence", "mp4", "webm"];

  const hasQuality = $derived(
    format === "gif" || format === "mp4" || format === "webm" || (format === "webp" && options.lossy),
  );
  const hasLossy = $derived(format === "gif" || format === "webp");
  const hasLoop = $derived(format === "gif" || format === "apng" || format === "webp");
  const outW = $derived(Math.max(1, Math.round(width * options.scale)));
  const outH = $derived(Math.max(1, Math.round(height * options.scale)));

  onMount(() => {
    recordingFormats().then((list) => (available = list));
    dialog?.focus();
  });

  // Re-estimate (debounced) whenever the choice changes.
  let timer: ReturnType<typeof setTimeout> | undefined;
  let run = 0;
  $effect(() => {
    const choice = { format, options: { ...options } };
    clearTimeout(timer);
    estimating = true;
    const id = ++run;
    timer = setTimeout(() => {
      editorEstimate(choice.format, choice.options)
        .then((bytes) => id === run && (estimate = bytes))
        .catch(() => id === run && (estimate = null))
        .finally(() => id === run && (estimating = false));
    }, 350);
    return () => clearTimeout(timer);
  });

  function size(bytes: number) {
    const units = ["B", "KB", "MB", "GB"];
    let value = bytes;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit++;
    }
    return `${value.toLocaleString(undefined, { maximumFractionDigits: value < 10 ? 1 : 0 })} ${units[unit]}`;
  }

  function onkeydown(e: KeyboardEvent) {
    e.stopPropagation(); // keep editor shortcuts out
    if (e.key === "Escape") oncancel();
  }

  const save = (ask: boolean) => onsave(format, { ...options, lossy: hasLossy && options.lossy }, ask);
</script>

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && oncancel()}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label={t("export.title")}
    tabindex="-1"
    bind:this={dialog}
    {onkeydown}
  >
    <h2>{t("export.title")}</h2>

    <div class="formats" role="radiogroup" aria-label={t("export.format")}>
      {#each order as f (f)}
        <button
          role="radio"
          aria-checked={format === f}
          class:active={format === f}
          disabled={!available.includes(f)}
          title={available.includes(f) ? t(`export.hint.${f}`) : t("settings.needsFfmpeg")}
          onclick={() => (format = f)}
        >
          {labels[f]}
        </button>
      {/each}
    </div>
    <p class="hint">{t(`export.hint.${format}`)}</p>

    <div class="grid">
      {#if hasLossy}
        <span>{t("export.compression")}</span>
        <label class="check">
          <input type="checkbox" bind:checked={options.lossy} disabled={format === "webp" && !ffmpeg} />
          {format === "gif" ? t("export.lossyGif") : t("export.lossyWebp")}
          {#if format === "webp" && !ffmpeg}<small>({t("settings.needsFfmpeg")})</small>{/if}
        </label>
      {/if}

      {#if hasQuality}
        <span>{t("export.quality")}</span>
        <label class="range">
          <input type="range" min="1" max="100" bind:value={options.quality} />
          <b>{options.quality}</b>
        </label>
      {/if}

      {#if hasLoop}
        <span>{t("export.loop")}</span>
        <label class="loop">
          <select
            value={options.loopCount === null ? "project" : options.loopCount === 0 ? "forever" : "count"}
            onchange={(e) => {
              const v = e.currentTarget.value;
              options.loopCount = v === "project" ? null : v === "forever" ? 0 : 1;
            }}
          >
            <option value="project">{t("export.loopProject")}</option>
            <option value="forever">{t("export.loopForever")}</option>
            <option value="count">{t("export.loopCount")}</option>
          </select>
          {#if options.loopCount !== null && options.loopCount > 0}
            <input type="number" min="1" max="1000" bind:value={options.loopCount} />
          {/if}
        </label>
      {/if}

      <span>{t("export.size")}</span>
      <label class="range">
        <input
          type="range"
          min="10"
          max="100"
          step="5"
          bind:value={() => Math.round(options.scale * 100), (v) => (options.scale = v / 100)}
        />
        <b>{Math.round(options.scale * 100)} % · {outW}×{outH}</b>
      </label>
    </div>

    <p class="estimate">
      {t("export.estimate")}
      <b>{estimating ? t("export.estimating") : estimate !== null ? `≈ ${size(estimate)}` : "–"}</b>
    </p>

    <div class="actions">
      <button onclick={oncancel}>{t("editor.cancel")}</button>
      <span class="spacer"></span>
      <button onclick={() => save(false)} title={t("export.saveToFolderTitle")}
        >{t("export.saveToFolder")}</button
      >
      <button class="primary" onclick={() => save(true)}>{t("export.saveAs")}</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.45);
  }
  .dialog {
    width: min(560px, calc(100vw - 32px));
    padding: 20px 22px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.35);
    outline: none;
  }
  h2 {
    margin: 0 0 14px;
    font-size: 1.1rem;
  }
  .formats {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  button {
    height: 32px;
    padding: 0 12px;
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
  .formats button.active {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
  }
  .hint {
    min-height: 2.6em;
    margin: 8px 0 14px;
    color: var(--text-muted);
    font-size: 0.85rem;
  }
  .grid {
    display: grid;
    grid-template-columns: max-content 1fr;
    align-items: center;
    gap: 12px 16px;
    font-size: 0.9rem;
  }
  .grid > span {
    color: var(--text-muted);
  }
  label {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .range input {
    flex: 1;
  }
  .range b {
    min-width: 7em;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
  select,
  .loop input {
    height: 30px;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: inherit;
    font: inherit;
  }
  .loop input {
    width: 80px;
  }
  small {
    color: var(--text-muted);
  }
  .estimate {
    margin: 16px 0;
    font-size: 0.9rem;
    color: var(--text-muted);
  }
  .estimate b {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .spacer {
    flex: 1;
  }
  .primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
  }
</style>
