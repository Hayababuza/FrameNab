/** Type-safe wrappers around the Rust commands (`src-tauri/src/commands`). */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { Annotation } from "$lib/bindings/Annotation";

export type Platform = "windows" | "macOs" | "linuxX11" | "linuxWayland" | "unknown";
export type ExportFormat = "gif" | "apng" | "webp" | "pngSequence" | "mp4" | "webm";
export type Action = "screenshot" | "record" | "editor" | "settings";

export interface AppInfo {
  version: string;
  platform: Platform;
  requiresPortal: boolean;
  ffmpegAvailable: boolean;
  exportFormats: ExportFormat[];
}

export const platformLabel: Record<Platform, string> = {
  windows: "Windows",
  macOs: "macOS",
  linuxX11: "Linux (X11)",
  linuxWayland: "Linux (Wayland)",
  unknown: "?",
};

export const appInfo = () => invoke<AppInfo>("app_info");
export const runAction = (action: Action) => invoke<void>("run_action", { action });

export interface CaptureInfo {
  id: number;
  width: number;
  height: number;
  /** Other apps' windows in image pixels, topmost first (empty on Wayland). */
  windows: { x: number; y: number; w: number; h: number }[];
}

/** Selection in image pixels. */
export interface Selection {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const captureInfo = () => invoke<CaptureInfo | null>("capture_info");
export const copySelection = (selection: Selection, annotations: Annotation[]) =>
  invoke<void>("copy_selection", { selection, annotations });
export const cancelCapture = () => invoke<void>("cancel_capture");

/** Saves the selection; `ask` opens a "Save as" dialog. Resolves to the path, or null if cancelled. */
export const saveSelection = (selection: Selection, annotations: Annotation[], ask: boolean) =>
  invoke<string | null>("save_selection", { selection, annotations, ask });

/** Pins the selection as a floating window; `scale` = image pixels per CSS pixel. */
export const pinSelection = (selection: Selection, annotations: Annotation[], scale: number) =>
  invoke<void>("pin_selection", { selection, annotations, scale });

export interface PinInfo {
  id: number;
  width: number;
  height: number;
  /** The window manager keeps the aspect ratio while resizing. */
  aspectLocked: boolean;
}

export const pinInfo = () => invoke<PinInfo | null>("pin_info");
export const copyPin = () => invoke<void>("copy_pin");
export const defaultSaveDirectory = () => invoke<string>("default_save_directory");

export async function loadPin(info: PinInfo): Promise<ImageBitmap> {
  const response = await fetch(convertFileSrc(`pin/${info.id}`, "framenab"));
  if (!response.ok) throw new Error(`pin ${info.id}: HTTP ${response.status}`);
  const pixels = new Uint8ClampedArray(await response.arrayBuffer());
  return createImageBitmap(new ImageData(pixels, info.width, info.height));
}

/** Loads the pixels of the current capture through the `framenab://` protocol. */
export async function loadCapture(
  info: CaptureInfo,
): Promise<{ bitmap: ImageBitmap; pixels: Uint8ClampedArray }> {
  const response = await fetch(convertFileSrc(String(info.id), "framenab"));
  if (!response.ok) throw new Error(`capture ${info.id}: HTTP ${response.status}`);
  const pixels = new Uint8ClampedArray(await response.arrayBuffer());
  const bitmap = await createImageBitmap(new ImageData(pixels, info.width, info.height));
  return { bitmap, pixels };
}
