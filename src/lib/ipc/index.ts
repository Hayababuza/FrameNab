/** Type-safe wrappers around the Rust commands (`src-tauri/src/commands`). */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { Annotation } from "$lib/bindings/Annotation";
import type { Effects } from "$lib/bindings/Effects";
import type { FrameOp } from "$lib/bindings/FrameOp";
import type { ExportOptions } from "$lib/stores/settings";
import type { Overlays } from "$lib/bindings/Overlays";
import type { ImageOp } from "$lib/bindings/ImageOp";
import type { TransitionOp } from "$lib/bindings/TransitionOp";
import type { InputEvents } from "$lib/bindings/InputEvents";
import type { Project } from "$lib/bindings/Project";

export type Platform = "windows" | "macOs" | "linuxX11" | "linuxWayland" | "unknown";
export type ExportFormat = "gif" | "apng" | "webp" | "pngSequence" | "mp4" | "webm";
export type Action = "screenshot" | "record" | "recordScreen" | "editor" | "settings";

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
  /** "record": the overlay chooses the area of a recording. */
  mode: "screenshot" | "record";
  /** A single window can be recorded via the system dialog (Wayland). */
  windowPicker: boolean;
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

// ---- recording -----------------------------------------------------------------------------------

/** Record mode: record the selection (image pixels), or the whole screen with `null`. */
export const recordSelection = (selection: Selection | null) =>
  invoke<void>("record_selection", { selection });
/** Record mode: record a window chosen in the system dialog. */
export const recordWindow = () => invoke<void>("record_window");

export interface RecorderSetup {
  /** In image pixels; null = a window chosen in the system dialog. */
  region: { x: number; y: number; w: number; h: number } | null;
  imageWidth: number;
  imageHeight: number;
  /** The frozen screen image is available via `loadRecorderBackground` (Wayland). */
  background: boolean;
  /** Wayland: shrink to a compact bar while recording (no transparent fullscreen there). */
  compact: boolean;
}

/** The frozen screen image of the area selection, shown behind the frame on Wayland. */
export async function loadRecorderBackground(setup: RecorderSetup): Promise<ImageBitmap> {
  const response = await fetch(convertFileSrc("recorder-background", "framenab"));
  if (!response.ok) throw new Error(`recorder background: HTTP ${response.status}`);
  const pixels = new Uint8ClampedArray(await response.arrayBuffer());
  return createImageBitmap(new ImageData(pixels, setup.imageWidth, setup.imageHeight));
}

export interface RecorderStatus {
  recording: boolean;
  paused: boolean;
  elapsedMs: number;
  error: string | null;
}

export interface RecordingSummary {
  path: string;
  frames: number;
  durationMs: number;
  width: number;
  height: number;
  dropped: number;
}

export const recorderInfo = () => invoke<RecorderSetup | null>("recorder_info");
export const recorderPrepare = (cursor: boolean) => invoke<void>("recorder_prepare", { cursor });
export const recorderStart = (fps: number) => invoke<void>("recorder_start", { fps });
export const recorderPause = (paused: boolean) => invoke<void>("recorder_pause", { paused });
export const recorderStatus = () => invoke<RecorderStatus>("recorder_status");
export const recorderStop = () => invoke<RecordingSummary>("recorder_stop");
/** Only these rects (CSS pixels) of the calling window take mouse input; the rest clicks through. */
export const setInputRegion = (rects: { x: number; y: number; w: number; h: number }[]) =>
  invoke<void>("set_input_region", { rects });

// ---- editor --------------------------------------------------------------------------------------

export interface EditorView {
  project: Project;
  path: string;
  canUndo: boolean;
  canRedo: boolean;
}

export const editorView = () => invoke<EditorView | null>("editor_view");
/** Applies a frame operation (delete, duplicate, reverse, timing …). */
export const editorFrames = (op: FrameOp) => invoke<EditorView>("editor_frames", { op });
/** Crops, resizes, rotates or flips all frames (progress: `image-progress` events). */
export const editorImage = (op: ImageOp) => invoke<EditorView>("editor_image", { op });
/** Inserts transition frames (fade, slide) after a frame. */
export const editorTransition = (op: TransitionOp) => invoke<EditorView>("editor_transition", { op });
export const editorUndo = (redo = false) => invoke<EditorView>("editor_undo", { redo });
export const editorSetAnnotations = (annotations: Annotation[]) =>
  invoke<EditorView>("editor_set_annotations", { annotations });
/** Saves the recording (into the save folder, or with `ask` via a "Save as" dialog); resolves to the
 * file path, or null if the dialog was cancelled. */
export const editorExport = (ask = false, format?: ExportFormat, options?: ExportOptions) =>
  invoke<string | null>("editor_export", { ask, format: format ?? null, options: options ?? null });
/** Estimated file size in bytes of saving the open project. */
export const editorEstimate = (format: ExportFormat, options: ExportOptions) =>
  invoke<number>("editor_estimate", { format, options });
export const editorCancelExport = () => invoke<void>("editor_cancel_export");
/** Formats recordings can be saved in on this system (MP4/WebM need FFmpeg). */
export const recordingFormats = () => invoke<ExportFormat[]>("recording_formats");

// Frame ids repeat across projects (`f000001` …), so the project path goes into the URL too;
// otherwise the webview shows cached images of the previously opened recording.
const frameUrl = (kind: string, frameId: string, project: string) =>
  `${convertFileSrc(`${kind}/${frameId}`, "framenab")}?p=${encodeURIComponent(project)}`;
export const thumbUrl = (frameId: string, project: string) => frameUrl("thumb", frameId, project);
export const previewUrl = (frameId: string, project: string) => frameUrl("preview", frameId, project);
export const openProject = (path: string) => invoke<void>("open_project", { path });
/** Exports the open project as `*.framenab.zip`; `null` if the dialog was cancelled. */
export const editorExportProject = () => invoke<string | null>("editor_export_project");
export const editorSetOverlays = (overlays: Overlays) =>
  invoke<EditorView>("editor_set_overlays", { overlays });
/** Picks an image and makes it the watermark; `null` if the dialog was cancelled. */
export const editorPickWatermark = () => invoke<EditorView | null>("editor_pick_watermark");
/** URL of an image in the open project's `assets/` folder. */
export const assetUrl = (file: string, project: string) =>
  `${convertFileSrc(`asset/${file}`, "framenab")}?p=${encodeURIComponent(project)}`;

export interface RecentProject {
  path: string;
  modifiedMs: number;
  frames: number;
  durationMs: number;
  width: number;
  height: number;
  /** The recording did not finish (crash); opening it recovers what was written. */
  interrupted: boolean;
}
/** The most recently changed recordings. */
export const recentProjects = (limit = 8) => invoke<RecentProject[]>("recent_projects", { limit });
/** Deletes a recording folder (only inside the recordings folder). */
export const deleteProject = (path: string) => invoke<void>("delete_project", { path });
export const recordingsDirectory = () => invoke<string>("recordings_directory");
/** Wayland: screen position (pixels) of the recorder's calibration marker, or null if not found. */
export const locateMarker = (scale: number) => invoke<[number, number] | null>("locate_marker", { scale });
export const editorInput = () => invoke<InputEvents>("editor_input");
export const editorSetEffects = (effects: Effects) => invoke<EditorView>("editor_set_effects", { effects });
/** Whether clicks and keys can be recorded (Linux needs the `input` group). */
export const inputSupport = () => invoke<"available" | "noPermission" | "unsupported">("input_support");

export interface HotkeyProblem {
  action: "screenshot" | "record";
  shortcut: string;
  error: string;
}
/** Registers the shortcuts saved in the settings; returns the ones that did not work. */
export const applyHotkeys = () => invoke<HotkeyProblem[]>("apply_hotkeys");
