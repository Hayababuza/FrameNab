/** Persistente Einstellungen über tauri-plugin-store (`settings.json` im App-Datenordner). */
import { load, type Store } from "@tauri-apps/plugin-store";
import type { Locale } from "$lib/i18n/translate";

export interface Settings {
  /** `null` = Systemsprache */
  locale: Locale | null;
}

const defaults: Settings = { locale: null };

let store: Promise<Store> | undefined;
const getStore = () => (store ??= load("settings.json", { defaults: { ...defaults }, autoSave: 200 }));

export async function getSetting<K extends keyof Settings>(key: K): Promise<Settings[K]> {
  return (await (await getStore()).get<Settings[K]>(key)) ?? defaults[key];
}

export async function setSetting<K extends keyof Settings>(key: K, value: Settings[K]): Promise<void> {
  await (await getStore()).set(key, value);
}
