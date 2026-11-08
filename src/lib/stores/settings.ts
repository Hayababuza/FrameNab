/** Persistent settings via tauri-plugin-store (`settings.json` in the app data folder). */
import { load, type Store } from "@tauri-apps/plugin-store";
import type { Locale } from "$lib/i18n/translate";

export type ImageFormat = "png" | "jpeg" | "webp";
export type Theme = "system" | "light" | "dark";
export type RecordingFormat = "gif" | "apng" | "webp" | "pngSequence" | "mp4" | "webm";

/** Settings of an animation export (mirrors `framenab_export::ExportOptions`). */
export interface ExportOptions {
  /** 1–100 */
  quality: number;
  /** GIF: lossy compression; WebP: lossy instead of lossless (needs FFmpeg). */
  lossy: boolean;
  /** Repetitions of GIF/APNG/WebP (0 = forever); `null` = the project's value. */
  loopCount: number | null;
  /** Output size relative to the project (0.1–1). */
  scale: number;
}

export interface Settings {
  /** `null` = system language */
  locale: Locale | null;
  theme: Theme;
  /** Folder for Ctrl+S; `null` = `<Pictures>/FrameNab`. */
  saveDirectory: string | null;
  /** File name with `{…}` date tokens: yyyy, yy, MM, dd, HH, mm, ss. */
  filenameTemplate: string;
  imageFormat: ImageFormat;
  recorderFps: number;
  /** Seconds before the recording starts. */
  recorderCountdown: number;
  recorderCursor: boolean;
  /** Format for saving recordings from the editor. */
  recordingFormat: RecordingFormat;
  /** Wayland session: use native Wayland windows instead of XWayland (applies after a restart). */
  nativeWayland: boolean;
  /** Record clicks and key presses during recordings (for the click and key effects). */
  recordInput: boolean;
  /** Last used export settings (also used by the quick "Save"). */
  exportOptions: ExportOptions;
  /** Global shortcuts as accelerators (`Shift+PrintScreen`); `null` = off. Not on Wayland. */
  hotkeys: { screenshot: string | null; record: string | null };
}

export const DEFAULT_HOTKEYS = { screenshot: "PrintScreen", record: "Shift+PrintScreen" };

export const DEFAULT_TEMPLATE = "FrameNab_{yyyy-MM-dd_HH-mm-ss}";

const defaults: Settings = {
  locale: null,
  theme: "system",
  saveDirectory: null,
  filenameTemplate: DEFAULT_TEMPLATE,
  imageFormat: "png",
  recorderFps: 15,
  recorderCountdown: 3,
  recorderCursor: true,
  recordingFormat: "gif",
  nativeWayland: false,
  recordInput: true,
  exportOptions: { quality: 90, lossy: false, loopCount: null, scale: 1 },
  hotkeys: { ...DEFAULT_HOTKEYS },
};

let store: Promise<Store> | undefined;
const getStore = () => (store ??= load("settings.json", { defaults: { ...defaults }, autoSave: 200 }));

export async function getSetting<K extends keyof Settings>(key: K): Promise<Settings[K]> {
  return (await (await getStore()).get<Settings[K]>(key)) ?? defaults[key];
}

export async function setSetting<K extends keyof Settings>(key: K, value: Settings[K]): Promise<void> {
  await (await getStore()).set(key, value);
}
