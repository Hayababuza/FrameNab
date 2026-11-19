<script lang="ts">
  /** The color read with the color picker: swatch, HEX and RGB (each copyable), "use for drawing". */
  import type { Rgba } from "$lib/bindings/Rgba";
  import { hexOf, rgbOf } from "$lib/annotations/inspect";
  import { t } from "$lib/i18n/index.svelte";

  interface Props {
    color: Rgba;
    /** Makes the color the drawing color. */
    onuse: (color: Rgba) => void;
  }

  let { color, onuse }: Props = $props();

  const hex = $derived(hexOf(color));
  const rgb = $derived(rgbOf(color));
  let copied = $state<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function copy(value: string) {
    try {
      await navigator.clipboard.writeText(value);
      copied = value;
      clearTimeout(timer);
      timer = setTimeout(() => (copied = null), 1500);
    } catch (e) {
      console.warn("copying the color failed:", e);
    }
  }
</script>

<div class="readout">
  <span class="swatch" style:background={hex}></span>
  {#each [hex, rgb] as value (value)}
    <button class="value" title={t("inspect.copy", { value })} onclick={() => copy(value)}>
      {copied === value ? t("inspect.copied") : value}
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M10 8h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-8a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z" />
        <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
      </svg>
    </button>
  {/each}
  <button class="use" title={t("inspect.useColorTitle")} onclick={() => onuse({ ...color, a: 255 })}>
    {t("inspect.useColor")}
  </button>
</div>

<style>
  .readout {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
  }
  .swatch {
    width: 22px;
    height: 22px;
    border-radius: 5px;
    border: 1px solid rgb(127 127 127 / 60%);
  }
  button {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 28px;
    padding: 0 8px;
    border: 1px solid rgb(127 127 127 / 35%);
    border-radius: 6px;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  button:hover {
    background: rgb(127 127 127 / 18%);
  }
  .value {
    min-width: 7.5em;
    justify-content: space-between;
    font-family: ui-monospace, monospace;
    font-variant-numeric: tabular-nums;
  }
  svg {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0.7;
  }
</style>
