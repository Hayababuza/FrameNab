# FrameNab – Project Plan

> **Status:** Approved on 2026-11-06; phase 0 is done. The decisions taken are listed in §11.

---

## 1. Vision

**FrameNab** is a cross-platform, open-source desktop tool that combines two tools:

1. **Quick annotated screenshots** (like Flameshot): shortcut → the screen freezes → select an area → annotate directly in the overlay → copy / save.
2. **Screen recording as GIF/video with a frame editor** (like ScreenToGif): select an area → record → edit, annotate and trim individual frames in the editor → export as GIF / APNG / WebP / MP4.

The key idea of the merge: **one shared annotation system** for both. Internally a screenshot is simply a project with exactly one frame. It can be opened in the full editor at any time, and a recording uses the same tools (arrow, text, blur …).

**No code** is taken from ScreenToGif (MS-PL) or Flameshot (GPLv3). Both serve only as functional inspiration; name, look and assets are original.

---

## 2. Tech stack

| Area | Choice | Rationale |
|---|---|---|
| App framework | **Tauri 2** | Native windows on Windows/macOS/Linux, small binaries (~10 MB instead of ~150 MB with Electron), tray, multiple windows, transparent windows, built-in bundler (.msi/.dmg/.deb/.AppImage). |
| Backend / core | **Rust** (stable, edition 2024) | Image processing and encoding must be fast; the best GIF encoder (gifski) is a Rust library; platform capture APIs are easy to reach from Rust. |
| Frontend / UI | **SvelteKit (Svelte 5) + TypeScript + Vite**, static adapter, one route per window | Little boilerplate, very fast for canvas-heavy UIs (a timeline with hundreds of thumbnails). |
| Drawing in the UI | **HTML Canvas 2D** (own small engine, no heavy library) | Full control; the same annotation data structure is rendered in Rust for export. |
| Rendering for export | **tiny-skia** + **cosmic-text** (Rust) | Pixel-accurate, platform-independent rendering of annotations into frames. |
| JS package manager | **pnpm** | Fast and space-efficient. |

**Why not …?**
- *Qt/C++ (like Flameshot):* proven, but development is much slower and the frame editor would be harder to build.
- *Electron:* too large and memory-hungry for a tool that runs in the tray all the time.
- *.NET/Avalonia (like ScreenToGif, but cross-platform):* possible, but the capture APIs would still need native code per OS, and GIF encoding would be weaker.

### Key Rust crates (to be validated with spikes)
- `xcap`: screenshots and monitor/window lists (Win/macOS/X11, partly Wayland)
- `ashpd`: xdg-desktop-portal (Wayland: Screenshot and ScreenCast portals, GlobalShortcuts)
- `pipewire`: reading frames from the Wayland ScreenCast stream
- `scap` *(candidate)*: video capture via ScreenCaptureKit (macOS) / Windows.Graphics.Capture (Windows)
- `image`, `qoi`: image processing and fast lossless frame storage
- `gifski`: high-quality GIF encoding
- `png` (APNG), `webp-animator` or `libwebp-sys`: animated PNG/WebP
- `arboard`: clipboard (images)
- `serde`, `serde_json`, `thiserror`, `tracing`
- Tauri plugins: `global-shortcut`, `dialog`, `fs`, `store` (settings), `single-instance`, `autostart`, `updater` (later)

### External dependency
- **FFmpeg** (optional, from the system): only for MP4/WebM export. GIF/APNG/WebP/PNG work without it. **Decided: not bundled; used if installed.**

---

## 3. Platform support & capture strategy

Capture is the hardest part, so everything sits behind a common trait (`framenab-capture`):

```rust
trait CaptureBackend {
    fn monitors(&self) -> Result<Vec<MonitorInfo>>;
    fn screenshot(&self, target: &CaptureTarget) -> Result<RgbaImage>;   // monitor / region / window
    fn start_recording(&self, target: &CaptureTarget, fps: u32, sink: Box<dyn FrameSink>)
        -> Result<Box<dyn RecordingHandle>>;
}
```

| OS | Screenshot | Recording | Global hotkeys | Notes |
|---|---|---|---|---|
| **Windows 10/11** | xcap (DXGI/GDI) | Windows.Graphics.Capture | Tauri global-shortcut ✅ | Mind per-monitor DPI. |
| **macOS 12.3+** | ScreenCaptureKit | ScreenCaptureKit | Tauri global-shortcut ✅ | Needs the "Screen Recording" permission; show an onboarding dialog on first launch. |
| **Linux X11** | xcap (XGetImage/XShm) | xcap in a loop / XShm | Tauri global-shortcut ✅ | Easiest case. |
| **Linux Wayland** (GNOME, KDE, wlroots) | **xdg-desktop-portal Screenshot** | **ScreenCast portal + PipeWire** | **GlobalShortcuts portal** where available, otherwise a CLI command bound to a key in the system | See below. |

**Wayland strategy:**
- Wayland forbids apps from reading the screen directly. Every capture goes through the portal; the user confirms once, then a *restore token* is stored so they are not asked every time.
- **Screenshot flow like Flameshot:** first get a fullscreen screenshot through the portal, then open our own borderless fullscreen window showing that frozen image and select/annotate there. This works everywhere, including where overlays over other apps are restricted.
- **Recording:** the ScreenCast portal provides a PipeWire stream of the whole monitor; Rust crops it to the selected region. The recording frame is shown as its own window, or alternatively just as a border around the region.
- **Hotkeys:** `framenab gui`, `framenab record` etc. as CLI commands (like `flameshot gui`); they are forwarded to the running instance. See [wayland.md](wayland.md) for binding them to a key in GNOME/KDE.

---

## 4. Features by phase

### Phase 0 – Foundation ✅
- Tauri 2 + Svelte 5 + TS scaffold, workspace structure (see §5)
- Git repo, `.gitignore`, `.editorconfig`, license, README, CONTRIBUTING
- Linting/formatting: `rustfmt`, `clippy -D warnings`, ESLint, Prettier, `svelte-check`
- GitHub Actions CI: build and tests on `ubuntu-latest`, `windows-latest`, `macos-latest`
- Tray, window management, CLI with single-instance forwarding, default hotkeys, DE/EN i18n
- **Still open:** capture spikes per OS (1 screenshot + 3 s recording) to confirm the crate choice; moves into phase 1
- **Acceptance:** `pnpm tauri dev` starts a window on all 3 OSes; CI is green.

### Phase 1 – Screenshot mode (the Flameshot part) → **v0.1** ✅ (implemented; acceptance on Windows/macOS still open)
- Tray icon with menu: *Screenshot*, *Record*, *Open editor*, *Settings*, *Quit*
- Global shortcuts (default `Print` for screenshot, `Shift+Print` for recording)
- Overlay: frozen image, dimmed background, drag to select a region, handles to adjust it, pixel magnifier with coordinates and color value, size display (W×H)
- Correct on multiple monitors and HiDPI (different scale factors per monitor)
- Window detection: hovering highlights the window under the cursor, a click selects it (where the OS allows it)
- **Annotation tools** (shared engine, see §6):
  - select/move, arrow, line, rectangle, ellipse (filled/outlined), freehand pen, highlighter
  - text (font, size, color, background)
  - numbered markers (counter 1, 2, 3 …)
  - pixelate and blur (area)
  - color picker, stroke width, undo/redo (Ctrl+Z / Ctrl+Y)
- Actions: **copy to clipboard** (Enter / Ctrl+C), **save** (Ctrl+S, filename template e.g. `FrameNab_{yyyy-MM-dd_HH-mm-ss}.png`), **open in editor**, pin (keep as a floating always-on-top window), cancel (Esc)
- Output: PNG, JPG, WebP
- **Acceptance:** a region screenshot with an arrow and text is in the clipboard less than 1 s after pressing Enter, on all 3 OSes and on GNOME Wayland.

### Phase 2 – Recording (the ScreenToGif recorder) → **v0.2**
- Recorder window: movable, resizable frame around the recording region, size fields (W×H), presets (e.g. 640×480, 1280×720)
- Controls: record / pause / stop, FPS (1–60, default 15), countdown (0–5 s)
- Optional: capture the mouse cursor, highlight clicks (circle at the click position, where the OS provides click events)
- Frames are streamed **to disk as QOI** (temporary project folder), not kept in RAM, so long recordings stay stable
- Frame delays come from real timestamps (variable frame rate)
- After stopping → open the editor automatically
- **Acceptance:** a 60 s recording at 15 fps in 1080p drops fewer than 5 % of frames and uses less than 500 MB of RAM.

### Phase 3 – Frame editor (the ScreenToGif editor) → **v0.3**
- Layout: toolbar at the top (ribbon/tabs), preview with zoom in the middle, a **timeline with thumbnails** at the bottom (virtualized, smooth even with 2000+ frames), properties panel on the right
- Playback: play/pause, loop, frame forward/back, total duration
- **Frame operations:** select (single, range with Shift, multiple with Ctrl), delete, duplicate, reverse order, yo-yo (forward + backward), remove every n-th frame, remove duplicate or near-identical frames (threshold), move by drag
- **Timing:** set the delay per frame or selection, increase/decrease by %, scale speed
- **Image:** crop, resize (Lanczos), rotate/flip, border, background/padding
- **Annotations over frame ranges:** each annotation has `frameStart..frameEnd`, so subtitles or arrows can be shown only on certain frames; tools as in phase 1, plus watermark (image) and progress bar
- **Transitions:** insert fade and slide between frames
- Save and load projects (`.framenab` folder or ZIP, see §6), recovery after a crash
- Undo/redo for every operation (snapshot history)

### Phase 4 – Export → **v0.4**
- **GIF** via gifski: quality (1–100), loop count, max colors, dithering, optional lossy mode
- **APNG**, **animated WebP** (lossless/lossy)
- **MP4 (H.264) / WebM (VP9)** via FFmpeg, if available
- **PNG sequence** (ZIP or folder)
- Export dialog with **size estimate** and progress bar, cancellable, runs on a background thread
- After export: show the file in the file manager or copy its path to the clipboard

### Phase 5 – Polish → **v1.0**
- Settings window: hotkeys, save location, filename template, default FPS, theme (light/dark/system), language, autostart
- **i18n:** German and English from the start (no hard-coded strings)
- Installers and packages: Windows `.msi` + NSIS `.exe`, macOS `.dmg` (universal: Intel + Apple Silicon), Linux `.deb`, `.rpm`, `.AppImage`; Flatpak later
- Release workflow in GitHub Actions (a `v*` tag builds every package and uploads them as a draft release)
- Auto-updater (Tauri updater, signed)
- Accessibility: keyboard control in the editor, sufficient contrast

### After v1.0 (goal: cover as much of ScreenToGif and Flameshot as possible)
- Webcam recording and sketch board mode (as in ScreenToGif)
- Audio for video export
- Upload (Imgur, own server), OCR, scrolling window screenshots
- Plugin system

---

## 5. Project structure

```
FrameNab/
├─ .github/workflows/        ci.yml (release.yml in phase 5)
├─ docs/                     PROJECT-PLAN.md (this document), ARCHITECTURE.md, wayland.md
├─ src/                      # Frontend (SvelteKit + TS)
│  ├─ lib/
│  │  ├─ annotations/        geometry, hit tests (canvas renderer and tools in phase 1)
│  │  ├─ bindings/           TypeScript types generated from Rust (ts-rs)
│  │  ├─ i18n/               de.json, en.json
│  │  ├─ ipc/                type-safe wrappers around Tauri commands and events
│  │  └─ stores/             settings, undo history
│  ├─ routes/                one SvelteKit route per window:
│  │  ├─ +page.svelte        main window (start, later editor)
│  │  ├─ overlay/            screenshot overlay
│  │  ├─ recorder/           recording frame + controls
│  │  └─ settings/
│  └─ app.css
├─ src-tauri/                # Rust app (thin layer: windows, tray, commands)
│  ├─ src/
│  │  ├─ main.rs, lib.rs
│  │  ├─ commands/           #[tauri::command] functions
│  │  ├─ cli.rs, hotkeys.rs, i18n.rs, logging.rs, tray.rs, windows.rs
│  ├─ icons/, capabilities/, tauri.conf.json, Cargo.toml
├─ crates/                   # Core logic, independent of Tauri and therefore testable
│  ├─ framenab-capture/      CaptureBackend trait + backends: windows, macos, x11, wayland
│  ├─ framenab-core/         project model, frame store (QOI), undo history, frame operations
│  ├─ framenab-render/       annotation rendering (tiny-skia, cosmic-text), blur/pixelate
│  └─ framenab-export/       GIF (gifski), APNG, WebP, FFmpeg integration
├─ Cargo.toml                (workspace)
├─ package.json, pnpm-lock.yaml, vite.config.ts, tsconfig.json, svelte.config.js
├─ .editorconfig, .gitignore, .prettierrc, eslint.config.js, rustfmt.toml
├─ LICENSE, README.md, CONTRIBUTING.md, CHANGELOG.md
```

**Rule:** `src-tauri` contains only glue code. The logic lives in `crates/*` and has unit tests that run without a GUI.

---

## 6. Data model (core)

See [ARCHITECTURE.md](ARCHITECTURE.md#project-folder-framenab) for the `project.json` format.

- Annotations stay **non-destructive** until export.
- The same JSON schema is used in TypeScript (UI preview) and Rust (export rendering). The TS types are **generated from Rust** with `ts-rs`, so the two sides never drift apart.
- Visual regression tests ensure the canvas renderer (UI) and tiny-skia (export) look practically the same.
- Frames never travel as Base64 over IPC: the UI loads them through a **custom protocol** (`framenab://frame/<project>/<id>?thumb=1`) served by Rust.

---

## 7. Quality & conventions

- **Tests:** Rust unit tests in every `crates/*`, Vitest for frontend logic (annotation geometry, undo stack), a Playwright smoke test that loads the editor window with a test project
- **CI:** every push and PR runs fmt check, clippy, svelte-check, ESLint, the tests and a build on all 3 OSes
- **Commits:** Conventional Commits (`feat:`, `fix:`, `chore:` …), in English
- **Branches:** `main` (stable), feature branches → PR
- **Logging:** `tracing` with a log file in the app data folder, switchable to debug
- **Errors:** users always see understandable (translated) messages, no panics on the UI path

---

## 8. License

**MIT** (decided).

---

## 9. Prerequisites

See the [README](../README.md#prerequisites). On Linux, `libgbm-dev` and `libpipewire-0.3-dev` are needed for Wayland capture.

---

## 10. Phase 0 outcome

1. Tauri 2 project with SvelteKit + TS, restructured into a Cargo workspace with `crates/*`
2. Tooling: rustfmt, clippy, ESLint, Prettier, svelte-check, Vitest, `.editorconfig`
3. Compiling skeletons for every crate, including the `CaptureBackend` trait and the project model with tests
4. Tray icon, start window, settings window, placeholders for overlay and recorder
5. README, LICENSE, CONTRIBUTING, CHANGELOG, `docs/ARCHITECTURE.md`, `docs/wayland.md`
6. GitHub Actions CI workflow
7. Local Git repository on `main`

Next: phase 1 (→ v0.1), each phase followed by a short review.

---

## 11. Decisions (2026-11-06)

- **Frontend:** Svelte (SvelteKit, Svelte 5)
- **License:** MIT
- **FFmpeg:** not bundled; used if installed
- **Languages:** German + English to start
- **Git:** local only at first
- **Scope:** if possible, *everything* from ScreenToGif and Flameshot should be achievable. The "After v1.0" section is therefore part of the roadmap, not an optional outlook.
- **Project language:** code, comments, docs and commit messages are in English; the UI is available in German and English.
