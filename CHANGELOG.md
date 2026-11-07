# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows [SemVer](https://semver.org/).

## [Unreleased]

### Added
- Wayland recorder shows the frame around the area while recording: the window switches from fullscreen to maximized (which can be transparent on GNOME), finds its position on screen by locating a color marker in a portal screenshot, and draws the frame and the control bar next to the area; if the marker is not found it falls back to the compact bar
- Editor: "Save as…" (Ctrl+Shift+S) picks location and format (GIF, MP4, WebM); the filmstrip only renders visible frames, images are served off the main thread and thumbnails are cached (no more stutter while scrolling); drag over frames to select several (auto-scrolls at the edges); the mouse wheel scrolls the filmstrip
- Editor for recordings (first version): opens automatically after a recording stops; preview with playback (Space, ←/→), a filmstrip of all frames with their durations, select frames (click, Shift/Ctrl+click, Ctrl+A) and delete them (Del) with undo/redo; "Save" writes the result into the save folder with the file name template, in the format chosen in the settings ("Format for recordings": GIF via gifski, MP4/WebM via FFmpeg), with progress and cancel
- "Open project" on the start page and `framenab edit <folder>` open a recording in the editor
- Wayland recorder: GNOME puts black behind transparent fullscreen windows, so before recording the recorder shows the frozen screen image with the frame and the control bar, and from "Record" on the window shrinks to a compact, draggable bar (countdown, timer, pause, stop, result) that leaves the live screen usable
- `framenab record-screen` opens the recorder for the whole screen right away
- Recorder (stage 2b): "Record" opens the selection overlay in record mode (drag an area, click a window on Windows/macOS/X11, "Whole screen", or "App window…" via the system dialog on Wayland); then a borderless, transparent recorder window shows a frame around the area and a control bar (FPS, countdown, mouse cursor, record, pause/resume, stop, timer); everything except the control bar is click-through; the Wayland screen-sharing choice is remembered in the settings; recordings are saved as project folders under the app data folder, with "Open folder" afterwards; "Record" from the tray or a shortcut stops a running recording
- Recording engine (stage 2a): streams the screen via the xdg-desktop-portal ScreenCast portal and PipeWire on Wayland (the screen choice is remembered with a restore token; cursor on/off) and via xcap on Windows, macOS and X11; a common sampler takes the latest frame at the chosen FPS, crops it to the region, excludes paused time and lets unchanged frames last longer; frames are written as QOI by a background writer with a small queue (dropped frames are counted)
- Recording spike: `cargo run --release -p framenab-capture --example record -- <dir> [seconds] [fps] [--cursor] [--region x,y,w,h] [--token-file f]` (60 s at 15 fps in 1080p on Wayland: 0 dropped frames, 126 MB peak memory)

## [0.1.0] - 2026-11-07

First version: the screenshot mode (phase 1) on Windows, macOS and Linux (X11 and Wayland).

### Added
- Corner resize handles on pin windows, always visible and centered on the image frame like on the overlay selection; the aspect ratio is kept while resizing (GTK geometry hints on Linux, snapping elsewhere)
- Close button (✕) and New screenshot button (Ctrl+N) in pin windows, shown on hover; open pins stay on screen and appear in the new screenshot, so several pins can be combined into one picture
- Screenshot mode (stage 1a): capture the screen, select an area in a fullscreen overlay and copy it to the clipboard (Enter, Ctrl+C or double-click; Esc or right-click cancels)
- Stage 1c: resize handles on the selection; pixel loupe with coordinates and color; save to file (Ctrl+S) and Save as (Ctrl+Shift+S) with folder, file name template and format in the settings; pin a screenshot as a floating window; window snapping on Windows, macOS and X11 (hover highlights a window, a click selects it)
- Annotation tools in the screenshot overlay (stage 1b): select/move, arrow, line, rectangle, ellipse, pen, highlighter, text, numbered markers, pixelate and blur, with 8 preset colors plus a custom color, three stroke widths, undo/redo and keyboard shortcuts; changing color/width restyles the selected annotation; double-click a text to edit it
- Renderer: text and counters via cosmic-text (system sans-serif font), real blur
- Capture backends: xdg-desktop-portal on Wayland, `xcap` on Windows, macOS and X11
- Capture spike: `cargo run -p framenab-capture --example screenshot`
- Project scaffold: Tauri 2, SvelteKit (Svelte 5), TypeScript, Cargo workspace
- `framenab-core`: project model, QOI frame store, frame operations (delete, duplicate, reverse, yo-yo, remove every n-th, timing), undo history
- `framenab-render`: rendering for line, arrow, rectangle, ellipse, pen, highlighter, pixelate
- `framenab-capture`: `CaptureBackend` trait and platform detection (including Wayland)
- `framenab-export`: export formats and FFmpeg detection
- Tray menu, single instance with command forwarding (`framenab gui` …), default shortcuts (except on Wayland)
- Start window, settings window (language), placeholders for overlay and recorder
- German and English UI
- CI for Linux, Windows and macOS

### Fixed
- Dev mode: windows sometimes opened without their component CSS ("failed to load virtual css module"), e.g. the loupe stuck in the top-left corner; component CSS is now injected through JavaScript
- Pinned screenshots showed a black window: the frontend percent-encodes the protocol path (`pin%2F3`), which the backend did not decode
- Pinning no longer brings back the main window on top of the new pin; the pin gets focus. The main window comes back once the last pin is closed (if it was hidden for the screenshot)
- Wayland: window buttons (minimize, maximize, close) did not react to clicks until the window had been maximized once (tao header bar workaround)
- Wayland: a second screenshot in the same session hung forever

[Unreleased]: https://github.com/Hayababuza/FrameNab/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Hayababuza/FrameNab/releases/tag/v0.1.0
