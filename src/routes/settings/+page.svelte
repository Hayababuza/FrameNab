<script lang="ts">
  import { emit } from "@tauri-apps/api/event";
  import {
    disable as disableAutostart,
    enable as enableAutostart,
    isEnabled as autostartEnabled,
  } from "@tauri-apps/plugin-autostart";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { i18n, locales, t, type Locale } from "$lib/i18n/index.svelte";
  import {
    appInfo,
    applyHotkeys,
    defaultSaveDirectory,
    inputSupport,
    oldRecordings,
    openPrintScreenSettings,
    printScreenOwner,
    recordingFormats,
    releasePrintScreen,
    runAction,
    type ExportFormat,
    type HotkeyProblem,
  } from "$lib/ipc";
  import ShortcutField from "$lib/components/ShortcutField.svelte";
  import UpdateNotice from "$lib/components/UpdateNotice.svelte";
  import { findUpdate, type Update } from "$lib/updater";
  import {
    DEFAULT_HOTKEYS,
    DEFAULT_INSTANT_GIF,
    DEFAULT_TEMPLATE,
    getSetting,
    type Hotkeys,
    type InstantGif,
    type ImageFormat,
    type RecordingFormat,
    setSetting,
    type Theme,
  } from "$lib/stores/settings";
  import { expandTemplate } from "$lib/util/filename";

  const names: Record<Locale, string> = { de: "Deutsch", en: "English" };
  const formats: { value: ImageFormat; label: string; ext: string }[] = [
    { value: "png", label: "PNG", ext: "png" },
    { value: "jpeg", label: "JPEG", ext: "jpg" },
    { value: "webp", label: "WebP", ext: "webp" },
  ];

  let locale = $state<Locale | "system">("system");
  let theme = $state<Theme>("system");
  let updateCheck = $state(true);
  let cleanup = $state({ enabled: false, days: 30 });
  let cleanupPreview = $state<{ count: number; bytes: number } | null>(null);
  const cleanupChoices = [1, 7, 14, 30, 90];
  let version = $state("");
  let updateState = $state<
    | { kind: "idle" }
    | { kind: "checking" }
    | { kind: "none" }
    | { kind: "found"; update: Update }
    | { kind: "error"; error: string }
  >({ kind: "idle" });
  let hotkeys = $state<Hotkeys>({ ...DEFAULT_HOTKEYS });
  const hotkeyRows = [
    ["screenshot", "settings.hotkeyScreenshot"],
    ["record", "settings.hotkeyRecord"],
    ["recordNow", "settings.hotkeyRecordNow"],
  ] as const;
  let hotkeyProblems = $state<HotkeyProblem[]>([]);
  /** Windows 11 takes the Print Screen key; `released`: switched off, effective after signing in. */
  let printScreen = $state<"windows" | "free" | "released" | null>(null);
  let printScreenError = $state<string | null>(null);
  // Windows only takes the plain key, not combinations like Shift+PrintScreen.
  const usesPrintScreen = $derived(Object.values(hotkeys).includes("PrintScreen"));
  let autostart = $state(false);
  let saveDirectory = $state<string | null>(null);
  let defaultDirectory = $state("");
  let template = $state(DEFAULT_TEMPLATE);
  let imageFormat = $state<ImageFormat>("png");
  let recordingFormat = $state<RecordingFormat>("gif");
  let afterRecording = $state<"editor" | "save">("editor");
  let instant = $state<InstantGif>({ ...DEFAULT_INSTANT_GIF });
  const saveInstant = () => setSetting("instantGif", $state.snapshot(instant));
  const INSTANT_FPS = [5, 10, 15, 20, 25, 30];
  const INSTANT_COUNTDOWNS = [0, 1, 2, 3, 5];
  const INSTANT_SIZES = [1, 0.75, 0.5, 0.25];
  let copyRecording = $state(true);
  let closeEditorAfterCopy = $state(true);
  let available = $state<ExportFormat[]>(["gif"]);
  const recordingLabels: Record<RecordingFormat, string> = {
    gif: "GIF",
    apng: "APNG",
    webp: "WebP",
    pngSequence: "PNG (ZIP)",
    mp4: "MP4 (H.264)",
    webm: "WebM (VP9)",
  };

  const example = $derived(
    `${expandTemplate(template, new Date())}.${formats.find((f) => f.value === imageFormat)?.ext}`,
  );

  let waylandSession = $state(false);
  let recordInput = $state(true);
  let inputBlocked = $state(false);
  let useXwayland = $state(true);
  let restartNeeded = $state(false);

  onMount(async () => {
    waylandSession = (await appInfo()).platform === "linuxWayland";
    recordInput = await getSetting("recordInput");
    inputBlocked = (await inputSupport()) === "noPermission";
    useXwayland = !(await getSetting("nativeWayland"));
    locale = (await getSetting("locale")) ?? "system";
    theme = await getSetting("theme");
    updateCheck = await getSetting("updateCheck");
    cleanup = await getSetting("recordingCleanup");
    version = (await appInfo()).version;
    // Shortcuts added in later versions are missing from older settings.
    hotkeys = { ...DEFAULT_HOTKEYS, ...(await getSetting("hotkeys")) };
    afterRecording = await getSetting("afterRecording");
    instant = { ...DEFAULT_INSTANT_GIF, ...(await getSetting("instantGif")) };
    copyRecording = await getSetting("copyRecording");
    closeEditorAfterCopy = await getSetting("closeEditorAfterCopy");
    printScreen = await printScreenOwner().catch(() => null);
    autostart = await autostartEnabled().catch(() => false);
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

  async function setHotkey(action: keyof Hotkeys, value: string | null) {
    hotkeys = { ...hotkeys, [action]: value };
    await setSetting("hotkeys", hotkeys);
    hotkeyProblems = await applyHotkeys();
  }

  async function releaseKey() {
    printScreenError = null;
    try {
      await releasePrintScreen();
      printScreen = "released";
    } catch (e) {
      printScreenError = String(e);
    }
  }

  // Show how many recordings the chosen age affects, so nothing disappears by surprise.
  $effect(() => {
    const days = cleanup.days;
    oldRecordings(days)
      .then((r) => (cleanupPreview = r))
      .catch(() => (cleanupPreview = null));
  });

  function saveCleanup() {
    setSetting("recordingCleanup", { ...cleanup });
  }

  const megabytes = (bytes: number) =>
    bytes >= 1024 ** 3
      ? `${(bytes / 1024 ** 3).toLocaleString(undefined, { maximumFractionDigits: 1 })} GB`
      : `${Math.round(bytes / 1024 ** 2).toLocaleString()} MB`;

  async function checkUpdate() {
    updateState = { kind: "checking" };
    try {
      const update = await findUpdate();
      updateState = update ? { kind: "found", update } : { kind: "none" };
    } catch (e) {
      updateState = { kind: "error", error: String(e) };
    }
  }

  async function changeTheme() {
    await setSetting("theme", theme);
    await emit("settings-changed");
  }

  async function toggleAutostart() {
    try {
      await (autostart ? enableAutostart() : disableAutostart());
    } finally {
      autostart = await autostartEnabled().catch(() => false);
    }
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
    <label class="row">
      <span>{t("settings.theme")}</span>
      <select bind:value={theme} onchange={changeTheme}>
        <option value="system">{t("settings.themeSystem")}</option>
        <option value="light">{t("settings.themeLight")}</option>
        <option value="dark">{t("settings.themeDark")}</option>
      </select>
    </label>
    <label class="row">
      <span>
        {t("settings.autostart")}
        <small class="help-inline">{t("settings.autostartHelp")}</small>
      </span>
      <input type="checkbox" bind:checked={autostart} onchange={toggleAutostart} />
    </label>
  </section>

  {#if waylandSession}
    <h2>{t("settings.system")}</h2>
    <section>
      <label class="row">
        <span>
          {t("settings.xwayland")}
          <small class="help-inline">{t("settings.xwaylandHelp")}</small>
        </span>
        <input
          type="checkbox"
          bind:checked={useXwayland}
          onchange={async () => {
            await setSetting("nativeWayland", !useXwayland);
            restartNeeded = true;
          }}
        />
      </label>
      {#if restartNeeded}
        <p class="help">{t("settings.restartNeeded")}</p>
      {/if}
    </section>
  {/if}

  <h2>{t("settings.updates")}</h2>
  <section>
    <label class="row">
      <span>{t("settings.updateAuto")}</span>
      <input
        type="checkbox"
        bind:checked={updateCheck}
        onchange={() => setSetting("updateCheck", updateCheck)}
      />
    </label>
    <div class="row">
      <span>
        {#if updateState.kind === "checking"}
          {t("settings.updateChecking")}
        {:else if updateState.kind === "none"}
          {t("settings.updateNone", { version })}
        {:else if updateState.kind === "error"}
          {t("settings.updateError", { error: updateState.error })}
        {:else}
          FrameNab {version}
        {/if}
      </span>
      <button onclick={checkUpdate} disabled={updateState.kind === "checking"}
        >{t("settings.updateCheck")}</button
      >
    </div>
    {#if updateState.kind === "found"}
      <div class="row"><UpdateNotice update={updateState.update} /></div>
    {/if}
  </section>

  <h2>{t("settings.hotkeys")}</h2>
  <section>
    {#if waylandSession}
      <p class="help">{t("settings.hotkeysWayland")}</p>
      <div class="row">
        <span>{t("settings.hotkeyScreenshot")}</span>
        <code>framenab gui</code>
      </div>
      <div class="row">
        <span>{t("settings.hotkeyRecord")}</span>
        <code>framenab record</code>
      </div>
      <div class="row">
        <span>{t("settings.hotkeyRecordNow")}</span>
        <code>framenab record-now</code>
      </div>
    {:else}
      {#each hotkeyRows as [action, title] (action)}
        <div class="row">
          <span>{t(title)}</span>
          <div class="dir">
            <ShortcutField value={hotkeys[action]} onchange={(v) => setHotkey(action, v)} />
            {#if hotkeys[action] !== DEFAULT_HOTKEYS[action]}
              <button onclick={() => setHotkey(action, DEFAULT_HOTKEYS[action])}
                >{t("settings.hotkeyReset")}</button
              >
            {/if}
          </div>
        </div>
      {/each}
      <p class="help">{t("settings.hotkeyHelp")}</p>
      {#each hotkeyProblems as problem (problem.action)}
        <p class="help warn">
          {t("settings.hotkeyTaken", { shortcut: problem.shortcut, error: problem.error })}
        </p>
      {/each}
      {#if usesPrintScreen && printScreen === "windows"}
        <p class="help warn">{t("settings.printScreenTaken")}</p>
        <div class="row">
          <span></span>
          <div class="dir">
            <button onclick={releaseKey}>{t("settings.printScreenRelease")}</button>
            <button onclick={() => openPrintScreenSettings()}>{t("settings.printScreenSettings")}</button>
          </div>
        </div>
        {#if printScreenError}<p class="help warn">{printScreenError}</p>{/if}
      {:else if printScreen === "released"}
        <p class="help">{t("settings.printScreenReleased")}</p>
      {/if}
    {/if}
  </section>

  <h2>{t("recorder.title")}</h2>
  <section>
    <label class="row">
      <span>
        {t("settings.recordInput")}
        <small class="help-inline">{t("settings.recordInputHelp")}</small>
      </span>
      <input
        type="checkbox"
        bind:checked={recordInput}
        onchange={() => setSetting("recordInput", recordInput)}
      />
    </label>
    {#if recordInput && inputBlocked}
      <p class="help">
        {t("settings.inputPermission")}<br />
        <code>sudo usermod -aG input $USER</code>
      </p>
    {/if}
    <label class="row">
      <span>
        {t("settings.afterRecording")}
        <small class="help-inline">{t("settings.afterRecordingHelp")}</small>
      </span>
      <select bind:value={afterRecording} onchange={() => setSetting("afterRecording", afterRecording)}>
        <option value="editor">{t("settings.afterRecordingEditor")}</option>
        <option value="save">{t("settings.afterRecordingSave")}</option>
      </select>
    </label>
    <label class="row">
      <span>
        {t("settings.copyRecording")}
        <small class="help-inline">{t("settings.copyRecordingHelp")}</small>
      </span>
      <input
        type="checkbox"
        bind:checked={copyRecording}
        onchange={() => setSetting("copyRecording", copyRecording)}
      />
    </label>
    <label class="row">
      <span>
        {t("settings.closeEditorAfterCopy")}
        <small class="help-inline">{t("settings.closeEditorAfterCopyHelp")}</small>
      </span>
      <input
        type="checkbox"
        bind:checked={closeEditorAfterCopy}
        onchange={() => setSetting("closeEditorAfterCopy", closeEditorAfterCopy)}
      />
    </label>
  </section>

  <h2>{t("instant.title")}</h2>
  <section>
    <p class="help">
      {t("instant.help")}
      {#if hotkeys.recordNow && !waylandSession}
        {t("instant.helpShortcut", { shortcut: hotkeys.recordNow })}
      {/if}
    </p>
    <label class="row">
      <span>{t("instant.format")}</span>
      <select bind:value={instant.format} onchange={saveInstant}>
        {#each Object.entries(recordingLabels) as [value, label] (value)}
          <option {value} disabled={!available.includes(value as ExportFormat)}>
            {label}{available.includes(value as ExportFormat) ? "" : ` – ${t("settings.needsFfmpeg")}`}
          </option>
        {/each}
      </select>
    </label>
    <label class="row">
      <span>{t("instant.fps")}</span>
      <select bind:value={instant.fps} onchange={saveInstant}>
        {#each INSTANT_FPS as f (f)}<option value={f}>{f} fps</option>{/each}
      </select>
    </label>
    <label class="row">
      <span>{t("instant.countdown")}</span>
      <select bind:value={instant.countdown} onchange={saveInstant}>
        {#each INSTANT_COUNTDOWNS as c (c)}<option value={c}>{t("recorder.seconds", { n: c })}</option>{/each}
      </select>
    </label>
    <label class="row">
      <span>{t("instant.size")}</span>
      <select bind:value={instant.scale} onchange={saveInstant}>
        {#each INSTANT_SIZES as size (size)}<option value={size}>{Math.round(size * 100)} %</option>{/each}
      </select>
    </label>
    <label class="row">
      <span>{t("instant.cursor")}</span>
      <input type="checkbox" bind:checked={instant.cursor} onchange={saveInstant} />
    </label>
    <label class="row">
      <span>
        {t("instant.keys")}
        {#if !recordInput || inputBlocked}<small class="help-inline">{t("recorder.keysOff")}</small>{/if}
      </span>
      <input
        type="checkbox"
        bind:checked={instant.keys}
        onchange={saveInstant}
        disabled={!recordInput || inputBlocked}
      />
    </label>
    <label class="row">
      <span>{t("instant.copy")}</span>
      <input type="checkbox" bind:checked={instant.copy} onchange={saveInstant} />
    </label>
    <div class="row">
      <span></span>
      <button onclick={() => runAction("recordNow")}>{t("instant.start")}</button>
    </div>
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
    <label class="row">
      <span>
        {t("settings.cleanup")}
        <small class="help-inline">{t("settings.cleanupHelp")}</small>
      </span>
      <input type="checkbox" bind:checked={cleanup.enabled} onchange={saveCleanup} />
    </label>
    <label class="row">
      <span>
        {t("settings.cleanupAfter")}
        {#if cleanupPreview}
          <small class="help-inline"
            >{cleanupPreview.count
              ? t("settings.cleanupPreview", {
                  count: cleanupPreview.count,
                  size: megabytes(cleanupPreview.bytes),
                })
              : t("settings.cleanupPreviewNone")}</small
          >
        {/if}
      </span>
      <select bind:value={cleanup.days} onchange={saveCleanup}>
        {#each cleanupChoices as days (days)}
          <option value={days}
            >{days === 1 ? t("settings.cleanupDay") : t("settings.cleanupDays", { n: days })}</option
          >
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
  .help-inline {
    display: block;
    color: var(--text-muted);
    font-size: 0.8rem;
  }
  input[type="checkbox"] {
    width: 18px;
    height: 18px;
  }
  .help {
    margin: 0;
    padding: 0 16px 12px;
    font-size: 0.85rem;
    color: var(--text-muted);
  }
  .help.warn {
    color: #e5484d;
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
