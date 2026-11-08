<script lang="ts">
  /** Offers an available update: version, notes on hover, install with progress. */
  import { t } from "$lib/i18n/index.svelte";
  import { installUpdate, type Update } from "$lib/updater";

  let { update }: { update: Update } = $props();

  let progress = $state<number | null | undefined>(undefined);
  let error = $state<string | null>(null);

  async function install() {
    error = null;
    progress = null;
    try {
      await installUpdate(update, (fraction) => (progress = fraction));
    } catch (e) {
      error = String(e);
      progress = undefined;
    }
  }
</script>

<div class="update" title={update.body ?? ""}>
  <span>{t("update.available", { version: update.version })}</span>
  {#if progress !== undefined}
    <span class="progress"
      >{progress === null
        ? t("update.downloading")
        : t("update.progress", { percent: Math.round(progress * 100) })}</span
    >
  {:else}
    <button onclick={install}>{t("update.install")}</button>
  {/if}
  {#if error}<span class="error">{t("update.failed", { error })}</span>{/if}
</div>

<style>
  .update {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    background: var(--surface);
  }
  button {
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--accent);
    border-radius: 6px;
    background: var(--accent);
    color: var(--accent-text);
    cursor: pointer;
  }
  .progress,
  .error {
    color: var(--text-muted);
    font-size: 0.9rem;
  }
  .error {
    color: #e5484d;
  }
</style>
