/** Persistent settings via tauri-plugin-store (`settings.json` in the app data folder). */
import { load, type Store } from "@tauri-apps/plugin-store";
import type { Rgba } from "$lib/bindings/Rgba";
import type { Locale } from "$lib/i18n/translate";

export type ImageFormat = "png" | "jpeg" | "webp";
export type Theme = "system" | "light" | "dark";
export type RecordingFormat = "gif" | "apng" | "webp" | "pngSequence" | "mp4" | "webm";

/** Settings of an animation export (mirrors `framenapper_export::ExportOptions`). */
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
  /** Folder for Ctrl+S; `null` = `<Pictures>/FrameNapper`. */
  saveDirectory: string | null;
  /** File name with `{…}` date tokens: yyyy, yy, MM, dd, HH, mm, ss. */
  filenameTemplate: string;
  imageFormat: ImageFormat;
  recorderFps: number;
  /** Seconds before the recording starts. */
  recorderCountdown: number;
  /** Draw the mouse pointer into the recording. */
  recorderCursor: boolean;
  /** Show the pressed keys in the recording (they are recorded either way, if allowed). */
  recorderKeys: boolean;
  /** After a recording: open it in the editor, or save it right away (format and name from here). */
  afterRecording: "editor" | "save";
  /** Put a saved recording on the clipboard as a file, to paste it with Ctrl+V. */
  copyRecording: boolean;
  /** Close the editor after "Copy to clipboard". */
  closeEditorAfterCopy: boolean;
  /** Format for saving recordings (editor "Save", and saving right away). */
  recordingFormat: RecordingFormat;
  /** Wayland session: use native Wayland windows instead of XWayland (applies after a restart). */
  nativeWayland: boolean;
  /** Record clicks and key presses during recordings (for the click and key effects). */
  recordInput: boolean;
  /** Last used export settings (also used by the quick "Save"). */
  exportOptions: ExportOptions;
  /** Delete recording folders not changed for `days` (off by default). */
  recordingCleanup: { enabled: boolean; days: number };
  /** Check for a new version when the app starts. */
  updateCheck: boolean;
  /** Instant GIF: choose an area, it records right away and is saved and copied when stopped. */
  instantGif: InstantGif;
  /** Global shortcuts as accelerators (`Shift+PrintScreen`); `null` = off. Not on Wayland. */
  hotkeys: Hotkeys;
  /** Last chosen color and stroke width (CSS pixels) for annotations, in the overlay and editor. */
  annotationStyle: { color: Rgba; width: number };
  /** A right click in the screenshot overlay cancels it (discarding the selection and drawings). */
  rightClickCancels: boolean;
}

/** Mirrors `InstantGif` in `src-tauri/src/recorder.rs`. */
export interface InstantGif {
  format: RecordingFormat;
  fps: number;
  /** Seconds between choosing the area and the start. */
  countdown: number;
  /** Draw the mouse pointer in. */
  cursor: boolean;
  /** Show the pressed keys. */
  keys: boolean;
  /** Output size relative to the area (0.1–1). */
  scale: number;
  /** Put the file on the clipboard. */
  copy: boolean;
}

export const DEFAULT_INSTANT_GIF: InstantGif = {
  format: "gif",
  fps: 15,
  countdown: 1,
  cursor: true,
  keys: true,
  scale: 1,
  copy: true,
};

export interface Hotkeys {
  screenshot: string | null;
  record: string | null;
  /** Start recording the last area right away. */
  recordNow: string | null;
}

export const DEFAULT_HOTKEYS: Hotkeys = {
  screenshot: "PrintScreen",
  record: "Shift+PrintScreen",
  recordNow: "Ctrl+Shift+PrintScreen",
};

export const DEFAULT_TEMPLATE = "FrameNapper_{yyyy-MM-dd_HH-mm-ss}";

const defaults: Settings = {
  locale: null,
  theme: "system",
  saveDirectory: null,
  filenameTemplate: DEFAULT_TEMPLATE,
  imageFormat: "png",
  recorderFps: 15,
  recorderCountdown: 3,
  recorderCursor: true,
  recorderKeys: true,
  afterRecording: "editor",
  copyRecording: true,
  closeEditorAfterCopy: true,
  recordingFormat: "gif",
  nativeWayland: false,
  recordInput: true,
  exportOptions: { quality: 90, lossy: false, loopCount: null, scale: 1 },
  hotkeys: { ...DEFAULT_HOTKEYS },
  updateCheck: true,
  recordingCleanup: { enabled: false, days: 30 },
  instantGif: { ...DEFAULT_INSTANT_GIF },
  annotationStyle: { color: { r: 0xff, g: 0x3b, b: 0x30, a: 255 }, width: 4 },
  rightClickCancels: true,
};

let store: Promise<Store> | undefined;
const getStore = () => (store ??= load("settings.json", { defaults: { ...defaults }, autoSave: 200 }));

export async function getSetting<K extends keyof Settings>(key: K): Promise<Settings[K]> {
  return (await (await getStore()).get<Settings[K]>(key)) ?? defaults[key];
}

export async function setSetting<K extends keyof Settings>(key: K, value: Settings[K]): Promise<void> {
  await (await getStore()).set(key, value);
}
