<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import {
    appInfo,
    defaultSaveDirectory,
    openProject,
    platformLabel,
    recordingsDirectory,
    runAction,
    type Action,
    type AppInfo,
  } from "$lib/ipc";
  import { getSetting } from "$lib/stores/settings";

  let info = $state<AppInfo>();
  let saveDirectory = $state("");

  async function loadSaveDirectory() {
    saveDirectory = (await getSetting("saveDirectory")) ?? (await defaultSaveDirectory());
  }

  /** "Open project": choose a recording folder (*.framenab) and open it in the editor. */
  async function pickProject() {
    const picked = await open({ directory: true, defaultPath: await recordingsDirectory() });
    if (typeof picked === "string") await openProject(picked);
  }

  onMount(() => {
    appInfo().then((i) => (info = i));
    loadSaveDirectory();
    const unlisten = listen("save-settings-changed", loadSaveDirectory);
    return () => void unlisten.then((fn) => fn());
  });

  const cards: {
    action: Action;
    icon: string;
    title: "home.screenshot" | "home.record" | "home.open";
    hint: "home.screenshotHint" | "home.recordHint" | "home.openHint";
    phase?: number;
  }[] = [
    {
      action: "screenshot",
      icon: "M4 7V5a1 1 0 0 1 1-1h2M17 4h2a1 1 0 0 1 1 1v2M20 17v2a1 1 0 0 1-1 1h-2M7 20H5a1 1 0 0 1-1-1v-2",
      title: "home.screenshot",
      hint: "home.screenshotHint",
    },
    {
      action: "record",
      icon: "M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8ZM12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Z",
      title: "home.record",
      hint: "home.recordHint",
    },
    {
      action: "editor",
      icon: "M4 6h16M4 12h16M4 18h10",
      title: "home.open",
      hint: "home.openHint",
    },
  ];
</script>

<main>
  <header>
    <div class="brand">
      <img src="/favicon.png" alt="" width="36" height="36" />
      <div>
        <h1>FrameNab</h1>
        <p>{t("app.tagline")}</p>
      </div>
    </div>
    <button class="ghost" onclick={() => runAction("settings")}>{t("home.settings")}</button>
  </header>

  <section class="cards">
    {#each cards as card (card.action)}
      <button
        class="card"
        disabled={card.phase !== undefined}
        onclick={() => (card.action === "editor" ? pickProject() : runAction(card.action))}
      >
        <svg viewBox="0 0 24 24" width="28" height="28" aria-hidden="true">
          <path d={card.icon} fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
        </svg>
        <strong>{t(card.title)}</strong>
        <span>{card.phase ? t("home.comingSoon", { phase: card.phase }) : t(card.hint)}</span>
      </button>
    {/each}
  </section>

  {#if info}
    {#if info.requiresPortal}
      <p class="notice">{t("info.wayland", { command: "framenab gui" })}</p>
    {/if}
    <dl>
      <dt>{t("info.version")}</dt>
      <dd>{info.version}</dd>
      <dt>{t("info.platform")}</dt>
      <dd>{platformLabel[info.platform]}</dd>
      <dt>{t("info.ffmpeg")}</dt>
      <dd>{info.ffmpegAvailable ? t("info.ffmpegYes") : t("info.ffmpegNo")}</dd>
      <dt>{t("info.saveDirectory")}</dt>
      <dd class="dir">
        <code>{saveDirectory}</code>
        <button class="link" onclick={() => runAction("settings")}>{t("info.change")}</button>
      </dd>
    </dl>
  {/if}
</main>

<style>
  .dir {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .link {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  main {
    max-width: 880px;
    margin: 0 auto;
    padding: 32px 24px;
    display: flex;
    flex-direction: column;
    gap: 28px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .brand {
    display: flex;
    gap: 14px;
    align-items: center;
  }
  h1 {
    margin: 0;
    font-size: 1.5rem;
  }
  .brand p {
    margin: 0;
    color: var(--text-muted);
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 16px;
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    padding: 20px;
    text-align: left;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    cursor: pointer;
    transition:
      background 0.15s,
      border-color 0.15s;
  }
  .card svg {
    color: var(--accent);
    margin-bottom: 6px;
  }
  .card span {
    color: var(--text-muted);
    font-size: 0.9rem;
  }
  .card:hover:not(:disabled) {
    background: var(--surface-hover);
    border-color: var(--accent);
  }
  .card:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .ghost {
    background: none;
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 6px 14px;
    cursor: pointer;
  }
  .ghost:hover {
    background: var(--surface-hover);
  }
  .notice {
    margin: 0;
    padding: 12px 16px;
    border-radius: var(--radius);
    background: var(--info-bg);
    color: var(--info-text);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 20px;
    margin: 0;
    font-size: 0.9rem;
  }
  dt {
    color: var(--text-muted);
  }
  dd {
    margin: 0;
  }
</style>
