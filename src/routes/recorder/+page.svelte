<script lang="ts">
  /**
   * Recorder: a borderless, transparent, fullscreen window. It shows a frame around the area and
   * the controls next to it; only the controls take mouse input, the rest is click-through so
   * the recorded apps stay usable.
   */
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { onMount, tick } from "svelte";
  import type { Rect } from "$lib/bindings/Rect";
  import { toolbarPosition } from "$lib/annotations/selection";
  import { t } from "$lib/i18n/index.svelte";
  import {
    recorderInfo,
    recorderPause,
    recorderPrepare,
    recorderStart,
    recorderStatus,
    recorderStop,
    setInputRegion,
    type RecorderSetup,
    type RecordingSummary,
  } from "$lib/ipc";
  import { getSetting, setSetting } from "$lib/stores/settings";

  type Phase = "setup" | "preparing" | "countdown" | "recording" | "saving" | "done";

  const FPS = [5, 10, 15, 20, 25, 30, 60];
  const COUNTDOWNS = [0, 1, 3, 5];
  /** Gap between the recorded area and the frame, so the frame is never in the recording. */
  const FRAME_GAP = 3;

  const win = getCurrentWindow();
  let setup = $state<RecorderSetup | null>(null);
  let view = $state({ w: window.innerWidth, h: window.innerHeight });
  let phase = $state<Phase>("setup");
  let paused = $state(false);
  let elapsedMs = $state(0);
  let countdown = $state(0);
  let error = $state<string | null>(null);
  let summary = $state<RecordingSummary | null>(null);

  let fps = $state(15);
  let delay = $state(3);
  let cursor = $state(true);

  let bar: HTMLDivElement | undefined = $state();
  let barSize = $state({ w: 0, h: 0 });
  let statusTimer: ReturnType<typeof setInterval> | undefined;

  /** Recorded area in CSS pixels (null = a window chosen in the system dialog). */
  const area = $derived.by((): Rect | null => {
    if (!setup?.region) return null;
    const scale = setup.imageWidth / view.w;
    const r = setup.region;
    return { x: r.x / scale, y: r.y / scale, w: r.w / scale, h: r.h / scale };
  });

  const barStyle = $derived.by(() => {
    if (area) {
      const p = toolbarPosition(
        {
          x: area.x - FRAME_GAP,
          y: area.y - FRAME_GAP,
          w: area.w + 2 * FRAME_GAP,
          h: area.h + 2 * FRAME_GAP,
        },
        barSize,
        view,
      );
      return `left: ${p.x}px; top: ${p.y}px;`;
    }
    return `left: ${(view.w - barSize.w) / 2}px; bottom: 48px;`;
  });

  onMount(() => {
    (async () => {
      [fps, delay, cursor] = await Promise.all([
        getSetting("recorderFps"),
        getSetting("recorderCountdown"),
        getSetting("recorderCursor"),
      ]);
      setup = await recorderInfo();
      if (!setup) return void win.close();
      await tick();
      await win.show();
      // Some compositors (GNOME/Wayland) ignore fullscreen requests for hidden windows.
      await win.setFullscreen(true);
      await win.setFocus();
      await updateInputRegion();
    })().catch((e) => (error = String(e)));

    const unlisten = listen("recorder-stop-requested", () => stop());
    return () => {
      clearInterval(statusTimer);
      void unlisten.then((fn) => fn());
    };
  });

  /** Only the control bar takes clicks; the rest of the window lets them through. */
  async function updateInputRegion() {
    await tick();
    const r = bar?.getBoundingClientRect();
    await setInputRegion(r ? [{ x: r.x, y: r.y, w: r.width, h: r.height }] : []);
  }

  $effect(() => {
    void [barSize.w, barSize.h, barStyle, phase];
    updateInputRegion();
  });

  // ---- recording -----------------------------------------------------------------------------

  async function record() {
    error = null;
    summary = null;
    phase = "preparing";
    try {
      await recorderPrepare(cursor);
    } catch (e) {
      error = String(e);
      phase = "setup";
      return;
    }
    phase = "countdown";
    for (countdown = delay; countdown > 0; countdown--) {
      await new Promise((resolve) => setTimeout(resolve, 1000));
      if (phase !== "countdown") return; // closed meanwhile
    }
    try {
      await recorderStart(fps);
    } catch (e) {
      error = String(e);
      phase = "setup";
      return;
    }
    phase = "recording";
    paused = false;
    elapsedMs = 0;
    statusTimer = setInterval(async () => {
      const status = await recorderStatus();
      elapsedMs = status.elapsedMs;
      if (status.error) error = status.error;
    }, 250);
  }

  async function togglePause() {
    paused = !paused;
    await recorderPause(paused);
  }

  async function stop() {
    if (phase !== "recording") return;
    clearInterval(statusTimer);
    phase = "saving";
    try {
      summary = await recorderStop();
      phase = "done";
    } catch (e) {
      error = String(e);
      phase = "setup";
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && (phase === "setup" || phase === "done")) win.close();
  }

  const time = (ms: number) => {
    const s = Math.floor(ms / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  };
</script>

<svelte:window {onkeydown} onresize={() => (view = { w: window.innerWidth, h: window.innerHeight })} />

{#if area}
  <div
    class="frame"
    class:live={phase === "recording" && !paused}
    class:paused={phase === "recording" && paused}
    style="left: {area.x - FRAME_GAP}px; top: {area.y - FRAME_GAP}px; width: {area.w +
      2 * FRAME_GAP}px; height: {area.h + 2 * FRAME_GAP}px;"
  ></div>
  {#if phase === "countdown" && countdown > 0}
    <div class="countdown" style="left: {area.x + area.w / 2}px; top: {area.y + area.h / 2}px;">
      {countdown}
    </div>
  {/if}
{/if}

<div class="bar" style={barStyle} bind:this={bar} bind:clientWidth={barSize.w} bind:clientHeight={barSize.h}>
  {#if phase === "setup" || phase === "preparing" || phase === "countdown"}
    {#if !area}
      <span class="note">{t("recorder.windowHint")}</span>
    {/if}
    <label title={t("recorder.fps")}>
      <select bind:value={fps} onchange={() => setSetting("recorderFps", fps)} disabled={phase !== "setup"}>
        {#each FPS as f (f)}<option value={f}>{f} fps</option>{/each}
      </select>
    </label>
    <label title={t("recorder.countdown")}>
      <svg viewBox="0 0 24 24" aria-hidden="true"
        ><path d="M12 21a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM12 9v4l2 2M9 2h6" /></svg
      >
      <select
        bind:value={delay}
        onchange={() => setSetting("recorderCountdown", delay)}
        disabled={phase !== "setup"}
      >
        {#each COUNTDOWNS as c (c)}<option value={c}>{t("recorder.seconds", { n: c })}</option>{/each}
      </select>
    </label>
    <label class="check" title={t("recorder.cursor")}>
      <input
        type="checkbox"
        bind:checked={cursor}
        onchange={() => setSetting("recorderCursor", cursor)}
        disabled={phase !== "setup"}
      />
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 3l13 7-6 1.5L9.5 18z" /></svg>
    </label>
    <span class="sep"></span>
    <button class="primary" onclick={record} disabled={phase !== "setup"}>
      <span class="dot"></span>
      {phase === "preparing" ? t("recorder.preparing") : t("recorder.start")}
    </button>
    <button
      class="icon"
      title={t("recorder.close")}
      aria-label={t("recorder.close")}
      onclick={() => win.close()}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
    </button>
  {:else if phase === "recording" || phase === "saving"}
    <span class="timer" class:blink={!paused}><span class="dot live"></span>{time(elapsedMs)}</span>
    <button onclick={togglePause} disabled={phase !== "recording"}>
      {#if paused}
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 5l12 7-12 7z" /></svg>{t("recorder.resume")}
      {:else}
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5v14M16 5v14" /></svg>{t("recorder.pause")}
      {/if}
    </button>
    <button class="primary" onclick={stop} disabled={phase !== "recording"}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6h12v12H6z" /></svg>
      {phase === "saving" ? t("recorder.saving") : t("recorder.stop")}
    </button>
  {:else if phase === "done" && summary}
    <span class="note">
      {t("recorder.done", {
        frames: summary.frames,
        duration: time(summary.durationMs),
        w: summary.width,
        h: summary.height,
      })}
      {#if summary.dropped > 0}· {t("recorder.dropped", { n: summary.dropped })}{/if}
    </span>
    <button onclick={() => summary && revealItemInDir(summary.path)}>{t("recorder.openFolder")}</button>
    <button onclick={() => (phase = "setup")}>{t("recorder.again")}</button>
    <button
      class="icon"
      title={t("recorder.close")}
      aria-label={t("recorder.close")}
      onclick={() => win.close()}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
    </button>
  {/if}
  {#if error}
    <span class="error">{error}</span>
  {/if}
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    overflow: hidden;
    background: transparent;
  }
  .frame {
    position: fixed;
    box-sizing: border-box;
    border: 2px dashed #ff6369;
    pointer-events: none;
  }
  .frame.live {
    border-style: solid;
    border-color: #e5484d;
  }
  .frame.paused {
    border-style: solid;
    border-color: #ffb224;
  }
  .countdown {
    position: fixed;
    transform: translate(-50%, -50%);
    display: grid;
    place-items: center;
    width: 120px;
    height: 120px;
    border-radius: 50%;
    background: rgb(20 20 24 / 80%);
    color: #fff;
    font:
      600 64px/1 system-ui,
      sans-serif;
    pointer-events: none;
  }
  .bar {
    position: fixed;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    max-width: calc(100vw - 16px);
    padding: 6px;
    border-radius: 10px;
    background: rgb(20 20 24 / 94%);
    box-shadow: 0 4px 16px rgb(0 0 0 / 40%);
    color: #fff;
    font:
      13px system-ui,
      sans-serif;
  }
  .note {
    padding: 0 6px;
    opacity: 0.85;
  }
  .error {
    flex-basis: 100%;
    padding: 4px 6px;
    border-radius: 6px;
    background: rgb(160 30 30 / 90%);
  }
  label {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 4px;
  }
  label.check {
    cursor: pointer;
  }
  select {
    height: 30px;
    padding: 0 6px;
    border: 1px solid rgb(255 255 255 / 20%);
    border-radius: 6px;
    background: rgb(255 255 255 / 8%);
    color: inherit;
    font: inherit;
  }
  select option {
    color: #000;
  }
  .sep {
    width: 1px;
    align-self: stretch;
    margin: 4px;
    background: rgb(255 255 255 / 15%);
  }
  button {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 10px;
    border: none;
    border-radius: 6px;
    background: rgb(255 255 255 / 10%);
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: rgb(255 255 255 / 18%);
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button.primary {
    background: #e5484d;
  }
  button.primary:hover:not(:disabled) {
    background: #f15a5f;
  }
  button.icon {
    width: 32px;
    padding: 0;
    justify-content: center;
    background: transparent;
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
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #fff;
  }
  .dot.live {
    background: #e5484d;
  }
  .timer {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 64px;
    padding: 0 8px;
    font-variant-numeric: tabular-nums;
  }
  .timer.blink .dot {
    animation: blink 1s steps(2) infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0.2;
    }
  }
</style>
