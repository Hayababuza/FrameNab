/** Persistent settings via tauri-plugin-store (`settings.json` in the app data folder). */
import { load, type Store } from "@tauri-apps/plugin-store";
import type { Locale } from "$lib/i18n/translate";

export type ImageFormat = "png" | "jpeg" | "webp";

export interface Settings {
  /** `null` = system language */
  locale: Locale | null;
  /** Folder for Ctrl+S; `null` = `<Pictures>/FrameNab`. */
  saveDirectory: string | null;
  /** File name with `{…}` date tokens: yyyy, yy, MM, dd, HH, mm, ss. */
  filenameTemplate: string;
  imageFormat: ImageFormat;
}

export const DEFAULT_TEMPLATE = "FrameNab_{yyyy-MM-dd_HH-mm-ss}";

const defaults: Settings = {
  locale: null,
  saveDirectory: null,
  filenameTemplate: DEFAULT_TEMPLATE,
  imageFormat: "png",
};

let store: Promise<Store> | undefined;
const getStore = () => (store ??= load("settings.json", { defaults: { ...defaults }, autoSave: 200 }));

export async function getSetting<K extends keyof Settings>(key: K): Promise<Settings[K]> {
  return (await (await getStore()).get<Settings[K]>(key)) ?? defaults[key];
}

export async function setSetting<K extends keyof Settings>(key: K, value: Settings[K]): Promise<void> {
  await (await getStore()).set(key, value);
}
