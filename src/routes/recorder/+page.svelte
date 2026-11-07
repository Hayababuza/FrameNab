<script lang="ts">
  /**
   * Recorder window. Three looks:
   * - Windows/macOS/X11: transparent fullscreen window with a frame around the area and the
   *   control bar; only the bar takes mouse input, the rest is click-through.
   * - Wayland, before recording: fullscreen with the frozen screen image (GNOME puts black behind
   *   transparent fullscreen windows), the frame and the control bar.
   * - Wayland, while recording: the window shrinks to a compact, draggable bar (countdown, timer,
   *   pause, stop) so the live screen is visible and usable.
   */
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { onMount, tick } from "svelte";
  import type { Rect } from "$lib/bindings/Rect";
  import { toolbarPosition } from "$lib/annotations/selection";
  import { t } from "$lib/i18n/index.svelte";
  import {
    loadRecorderBackground,
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
  /** The window is a small bar instead of covering the screen (Wayland while recording). */
  let compact = $state(false);
  let paused = $state(false);
  let elapsedMs = $state(0);
  let countdown = $state(0);
  let error = $state<string | null>(null);
  let summary = $state<RecordingSummary | null>(null);

  let fps = $state(15);
  let delay = $state(3);
  let cursor = $state(true);

  let canvas: HTMLCanvasElement | undefined = $state();
  let background: ImageBitmap | null = null;
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

  const frame = $derived(
    area
      ? { x: area.x - FRAME_GAP, y: area.y - FRAME_GAP, w: area.w + 2 * FRAME_GAP, h: area.h + 2 * FRAME_GAP }
      : null,
  );

  const barStyle = $derived.by(() => {
    if (compact) return "left: 0; top: 0;";
    if (frame) {
      const p = toolbarPosition(frame, barSize, view);
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
      if (setup.background) {
        background = await loadRecorderBackground(setup);
        drawBackground();
      }
      await tick();
      await win.show();
      if (setup.compact && !setup.background) {
        // Nothing to show behind a frame (e.g. a window chosen in the system dialog): bar only.
        await enterCompact();
      } else {
        // Some compositors (GNOME/Wayland) ignore fullscreen requests for hidden windows.
        await win.setFullscreen(true);
      }
      await win.setFocus();
      await updateInputRegion();
    })().catch((e) => (error = String(e)));

    const unlisten = listen("recorder-stop-requested", () => stop());
    return () => {
      clearInterval(statusTimer);
      void unlisten.then((fn) => fn());
    };
  });

  /** Wayland setup view: the frozen screen, dimmed outside the area. */
  function drawBackground() {
    if (!canvas || !background) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(view.w * dpr);
    canvas.height = Math.round(view.h * dpr);
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.drawImage(background, 0, 0, view.w, view.h);
    ctx.fillStyle = "rgba(0, 0, 0, 0.5)";
    ctx.beginPath();
    ctx.rect(0, 0, view.w, view.h);
    if (area) ctx.rect(area.x, area.y, area.w, area.h);
    ctx.fill("evenodd");
  }

  $effect(() => {
    void [view, area, canvas];
    drawBackground();
  });

  /**
   * Which parts of the window take mouse input: only the bar over the live screen (transparent
   * mode), everything when the window shows the frozen image or is just the compact bar.
   */
  async function updateInputRegion() {
    await tick();
    if (compact || (setup?.background && !compact)) {
      await setInputRegion([{ x: 0, y: 0, w: window.innerWidth, h: window.innerHeight }]);
      return;
    }
    const r = bar?.getBoundingClientRect();
    await setInputRegion(r ? [{ x: r.x, y: r.y, w: r.width, h: r.height }] : []);
  }

  $effect(() => {
    void [barSize.w, barSize.h, barStyle, phase, compact];
    updateInputRegion();
    if (compact) fitWindowToBar();
  });

  async function enterCompact() {
    compact = true;
    await tick();
    await win.setFullscreen(false);
    await fitWindowToBar();
  }

  async function fitWindowToBar() {
    await tick();
    if (!bar) return;
    const r = bar.getBoundingClientRect();
    await win.setSize(new LogicalSize(Math.ceil(r.width), Math.ceil(r.height)));
  }

  /** In the compact bar, dragging the empty space moves the window. */
  function onbarmousedown(e: MouseEvent) {
    if (!compact || e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button, select, input, label")) return;
    win.startDragging();
  }

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
    // Wayland: show the live screen; the countdown runs in the bar, so there is time to drag the
    // bar out of the recorded area.
    if (setup?.compact && !compact) await enterCompact();
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
      // The recording continues in the editor (opened by the backend).
      await win.close();
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

{#if setup?.background && !compact}
  <canvas class="background" bind:this={canvas}></canvas>
{/if}

{#if frame && !compact}
  <div
    class="frame"
    class:live={phase === "recording" && !paused}
    class:paused={phase === "recording" && paused}
    style="left: {frame.x}px; top: {frame.y}px; width: {frame.w}px; height: {frame.h}px;"
  ></div>
  {#if area && phase === "countdown" && countdown > 0}
    <div class="countdown" style="left: {area.x + area.w / 2}px; top: {area.y + area.h / 2}px;">
      {countdown}
    </div>
  {/if}
{/if}

<div
  class="bar"
  class:compact
  style={barStyle}
  bind:this={bar}
  bind:clientWidth={barSize.w}
  bind:clientHeight={barSize.h}
  role="toolbar"
  tabindex="-1"
  onmousedown={onbarmousedown}
>
  {#if compact}
    <span class="grip" aria-hidden="true">⋮⋮</span>
  {/if}
  {#if phase === "countdown" && compact}
    <span class="count">{countdown}</span>
  {:else if phase === "setup" || phase === "preparing" || phase === "countdown"}
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
  .background {
    position: fixed;
    inset: 0;
    width: 100vw;
    height: 100vh;
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
    outline: none;
  }
  .bar.compact {
    flex-wrap: nowrap;
    max-width: none;
    width: max-content;
    box-shadow: none;
    cursor: move;
  }
  .grip {
    padding: 0 2px;
    opacity: 0.5;
    letter-spacing: -2px;
    user-select: none;
  }
  .count {
    min-width: 48px;
    text-align: center;
    font:
      600 22px/32px system-ui,
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
