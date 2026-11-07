<script lang="ts">
  import { emit } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { i18n, locales, t, type Locale } from "$lib/i18n/index.svelte";
  import { defaultSaveDirectory, recordingFormats, type ExportFormat } from "$lib/ipc";
  import {
    DEFAULT_TEMPLATE,
    getSetting,
    type ImageFormat,
    type RecordingFormat,
    setSetting,
  } from "$lib/stores/settings";
  import { expandTemplate } from "$lib/util/filename";

  const names: Record<Locale, string> = { de: "Deutsch", en: "English" };
  const formats: { value: ImageFormat; label: string; ext: string }[] = [
    { value: "png", label: "PNG", ext: "png" },
    { value: "jpeg", label: "JPEG", ext: "jpg" },
    { value: "webp", label: "WebP", ext: "webp" },
  ];

  let locale = $state<Locale | "system">("system");
  let saveDirectory = $state<string | null>(null);
  let defaultDirectory = $state("");
  let template = $state(DEFAULT_TEMPLATE);
  let imageFormat = $state<ImageFormat>("png");
  let recordingFormat = $state<RecordingFormat>("gif");
  let available = $state<ExportFormat[]>(["gif"]);
  const recordingLabels: Record<RecordingFormat, string> = {
    gif: "GIF",
    mp4: "MP4 (H.264)",
    webm: "WebM (VP9)",
  };

  const example = $derived(
    `${expandTemplate(template, new Date())}.${formats.find((f) => f.value === imageFormat)?.ext}`,
  );

  onMount(async () => {
    locale = (await getSetting("locale")) ?? "system";
    saveDirectory = await getSetting("saveDirectory");
    template = await getSetting("filenameTemplate");
    imageFormat = await getSetting("imageFormat");
    defaultDirectory = await defaultSaveDirectory();
    recordingFormat = await getSetting("recordingFormat");
    available = await recordingFormats();
  });

  async function changeLocale() {
    const value = locale === "system" ? null : locale;
    await setSetting("locale", value);
    i18n.set(value);
    await emit("settings-changed", { locale: value });
  }

  async function chooseDirectory() {
    const picked = await open({ directory: true, defaultPath: saveDirectory ?? defaultDirectory });
    if (typeof picked === "string") {
      saveDirectory = picked;
      await setSetting("saveDirectory", picked);
      await emit("save-settings-changed");
    }
  }

  async function resetDirectory() {
    saveDirectory = null;
    await setSetting("saveDirectory", null);
    await emit("save-settings-changed");
  }
</script>

<main>
  <h1>{t("settings.title")}</h1>

  <section>
    <label class="row">
      <span>{t("settings.language")}</span>
      <select bind:value={locale} onchange={changeLocale}>
        <option value="system">{t("settings.languageSystem")}</option>
        {#each locales as l (l)}
          <option value={l}>{names[l]}</option>
        {/each}
      </select>
    </label>
  </section>

  <h2>{t("settings.saving")}</h2>
  <section>
    <div class="row">
      <span>{t("settings.saveDirectory")}</span>
      <div class="dir">
        <code title={saveDirectory ?? defaultDirectory}>{saveDirectory ?? defaultDirectory}</code>
        <button onclick={chooseDirectory}>{t("settings.choose")}</button>
        {#if saveDirectory}
          <button onclick={resetDirectory}>{t("settings.reset")}</button>
        {/if}
      </div>
    </div>
    <label class="row">
      <span>{t("settings.filenameTemplate")}</span>
      <input
        type="text"
        bind:value={template}
        spellcheck="false"
        onchange={() => setSetting("filenameTemplate", template.trim() || DEFAULT_TEMPLATE)}
      />
    </label>
    <p class="help">
      {t("settings.filenameHelp")}<br />
      {t("settings.preview")}: <code>{example}</code>
    </p>
    <label class="row">
      <span>{t("settings.imageFormat")}</span>
      <select bind:value={imageFormat} onchange={() => setSetting("imageFormat", imageFormat)}>
        {#each formats as f (f.value)}
          <option value={f.value}>{f.label}</option>
        {/each}
      </select>
    </label>
    <label class="row">
      <span>{t("settings.recordingFormat")}</span>
      <select bind:value={recordingFormat} onchange={() => setSetting("recordingFormat", recordingFormat)}>
        {#each Object.entries(recordingLabels) as [value, label] (value)}
          <option {value} disabled={!available.includes(value as ExportFormat)}>
            {label}{available.includes(value as ExportFormat) ? "" : ` – ${t("settings.needsFfmpeg")}`}
          </option>
        {/each}
      </select>
    </label>
  </section>
</main>

<style>
  main {
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  h1 {
    margin: 0;
    font-size: 1.3rem;
  }
  h2 {
    margin: 8px 0 0;
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
  }
  section {
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 16px;
  }
  .row + .row,
  .help + .row {
    border-top: 1px solid var(--border);
  }
  .dir {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .dir code {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 280px;
  }
  .help {
    margin: 0;
    padding: 0 16px 12px;
    font-size: 0.85rem;
    color: var(--text-muted);
  }
  input,
  select,
  button {
    padding: 4px 8px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--bg);
    font: inherit;
    color: inherit;
  }
  input {
    width: 300px;
    font-family: ui-monospace, monospace;
    font-size: 0.9rem;
  }
  button {
    cursor: pointer;
    white-space: nowrap;
  }
  button:hover {
    background: var(--surface-hover);
  }
</style>
