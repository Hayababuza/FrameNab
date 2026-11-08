<script lang="ts">
  /**
   * Shows a global shortcut (accelerator like `Shift+PrintScreen`) and records a new one: click,
   * then press the keys. Esc cancels, Backspace/Delete switches the shortcut off.
   */
  import { t } from "$lib/i18n/index.svelte";

  interface Props {
    value: string | null;
    onchange: (value: string | null) => void;
  }
  let { value, onchange }: Props = $props();

  let recording = $state(false);

  const MODIFIER_CODES = /^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/;

  /** `Ctrl+Shift+KeyA` → `Strg + Umschalt + A`. */
  function label(accelerator: string) {
    const names: Record<string, string> = {
      Ctrl: t("keys.ctrl"),
      Shift: t("keys.shift"),
      Alt: "Alt",
      Super: t("keys.super"),
      PrintScreen: t("keys.print"),
    };
    return accelerator
      .split("+")
      .map((part) => names[part] ?? part.replace(/^Key/, "").replace(/^Digit/, ""))
      .join(" + ");
  }

  function accelerator(e: KeyboardEvent) {
    const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(
      Boolean,
    );
    return [...mods, e.code].join("+");
  }

  function onkey(e: KeyboardEvent) {
    if (!recording) return;
    // PrintScreen only arrives as keyup on some systems.
    if (e.type === "keyup" && e.code !== "PrintScreen") return;
    e.preventDefault();
    e.stopPropagation();
    if (MODIFIER_CODES.test(e.code)) return;
    recording = false;
    if (e.code === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey) return;
    if (e.code === "Backspace" || e.code === "Delete") return onchange(null);
    onchange(accelerator(e));
  }
</script>

<button
  class="shortcut"
  class:recording
  onclick={() => (recording = !recording)}
  onkeydown={onkey}
  onkeyup={onkey}
  onblur={() => (recording = false)}
>
  {recording ? t("keys.press") : value ? label(value) : t("keys.none")}
</button>

<style>
  .shortcut {
    min-width: 180px;
    height: 32px;
    padding: 0 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .shortcut.recording {
    border-color: var(--accent);
    color: var(--accent);
  }
</style>
