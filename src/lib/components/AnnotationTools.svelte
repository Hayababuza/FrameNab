<script lang="ts">
  /** Tool, color and stroke-width picker for annotations (used by the editor). */
  import type { Rgba } from "$lib/bindings/Rgba";
  import { colors, fromHex, toHex, type Tool, tools, widths } from "$lib/annotations/tools";
  import { t } from "$lib/i18n/index.svelte";

  interface Props {
    tool: Tool;
    color: Rgba;
    width: number;
    /** Called when the user picks a color or width (e.g. to restyle the selected annotation). */
    onstyle?: (change: { color?: Rgba; width?: number }) => void;
    disabled?: boolean;
  }

  let {
    tool = $bindable(),
    color = $bindable(),
    width = $bindable(),
    onstyle,
    disabled = false,
  }: Props = $props();

  const same = (a: Rgba, b: Rgba) => a.r === b.r && a.g === b.g && a.b === b.b;

  function pickColor(c: Rgba) {
    color = c;
    onstyle?.({ color: c });
  }

  function pickWidth(w: number) {
    width = w;
    onstyle?.({ width: w });
  }
</script>

<div class="tools" class:disabled>
  {#each tools as item (item.id)}
    <button
      class="icon"
      class:active={tool === item.id}
      title="{t(`tool.${item.id}`)} ({item.key.toUpperCase()})"
      aria-label={t(`tool.${item.id}`)}
      onclick={() => (tool = item.id)}
      {disabled}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d={item.icon} /></svg>
    </button>
  {/each}
  <span class="sep"></span>
  {#each colors as c (toHex(c))}
    <button
      class="swatch"
      class:active={same(c, color)}
      style:background={toHex(c)}
      aria-label={toHex(c)}
      onclick={() => pickColor(c)}
      {disabled}
    ></button>
  {/each}
  <label class="swatch custom" title={t("overlay.customColor")}>
    <input
      type="color"
      value={toHex(color)}
      oninput={(e) => pickColor(fromHex(e.currentTarget.value))}
      {disabled}
    />
  </label>
  <span class="sep"></span>
  {#each widths as w (w)}
    <button
      class="icon"
      class:active={width === w}
      aria-label="{t('overlay.width')} {w}"
      title="{t('overlay.width')} {w}"
      onclick={() => pickWidth(w)}
      {disabled}
    >
      <span class="dot" style:width="{w + 2}px" style:height="{w + 2}px"></span>
    </button>
  {/each}
</div>

<style>
  .tools {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px;
  }
  .tools.disabled {
    opacity: 0.5;
  }
  button {
    display: grid;
    place-items: center;
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  button.icon {
    width: 30px;
    height: 30px;
    padding: 0;
  }
  button:hover:not(:disabled) {
    background: var(--surface-hover);
  }
  button.active {
    border-color: var(--border);
    background: var(--surface-hover);
  }
  button:disabled {
    cursor: default;
  }
  svg {
    width: 17px;
    height: 17px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .sep {
    width: 1px;
    height: 22px;
    margin: 0 6px;
    background: var(--border);
  }
  .swatch {
    width: 20px;
    height: 20px;
    margin: 0 2px;
    padding: 0;
    border-radius: 50%;
    border: 2px solid var(--border);
  }
  .swatch.active {
    border-color: var(--text);
  }
  .swatch.custom {
    position: relative;
    overflow: hidden;
    cursor: pointer;
    background: conic-gradient(red, yellow, lime, cyan, blue, magenta, red);
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
</style>
