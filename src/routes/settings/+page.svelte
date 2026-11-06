<script lang="ts">
  import { emit } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { i18n, locales, t, type Locale } from "$lib/i18n/index.svelte";
  import { getSetting, setSetting } from "$lib/stores/settings";

  const names: Record<Locale, string> = { de: "Deutsch", en: "English" };
  let locale = $state<Locale | "system">("system");

  onMount(async () => {
    locale = (await getSetting("locale")) ?? "system";
  });

  async function changeLocale() {
    const value = locale === "system" ? null : locale;
    await setSetting("locale", value);
    i18n.set(value);
    await emit("settings-changed", { locale: value });
  }
</script>

<main>
  <h1>{t("settings.title")}</h1>
  <label>
    <span>{t("settings.language")}</span>
    <select bind:value={locale} onchange={changeLocale}>
      <option value="system">{t("settings.languageSystem")}</option>
      {#each locales as l (l)}
        <option value={l}>{names[l]}</option>
      {/each}
    </select>
  </label>
</main>

<style>
  main {
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  h1 {
    margin: 0;
    font-size: 1.3rem;
  }
  label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  select {
    padding: 4px 8px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--bg);
  }
</style>
