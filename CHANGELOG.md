# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows [SemVer](https://semver.org/).

## [Unreleased]

### Fixed
- Installed app on GNOME: the first screenshot failed ("Portal request didn't succeed") because GNOME only lets the focused app ask for the screenshot permission; FrameNab now comes to the front, explains the system question and asks again, so the permission is granted once and stored

## [0.4.0] - 2026-11-08

Export (phase 4) and polish (phase 5): more formats with an export dialog, overlays, project archives, crash recovery, settings for appearance, autostart and shortcuts, installers for every system and automatic updates.

### Added
- Updates: FrameNab checks the GitHub releases at startup (can be switched off) and in Settings → Updates; an available version is installed with one click (signed packages, also for `.deb`/`.rpm` and the AppImage) and the app restarts
- Installers: release workflow (`v*` tag) that builds `.exe`/`.msi` for Windows, a universal `.dmg` for macOS and `.deb`/`.rpm`/`.AppImage` for Linux into a draft GitHub release; package metadata (publisher, license, description, PipeWire dependency, per-user Windows setup in English and German)
- App icon: viewfinder corners with a red record dot (replaces the Tauri default icon)
- Settings: appearance (system, light, dark) for all windows including the title bars; start with the system (in the tray, via the autostart plugin); configurable global shortcuts for screenshot and recording (click and press the keys, Del switches off, shortcuts taken by other programs are reported); on Wayland the page shows the commands to bind in the system settings instead
- Export (phase 4): new formats APNG, animated WebP (lossless built in; lossy through FFmpeg) and PNG sequence (ZIP), next to GIF, MP4 and WebM. "Save as…" (Ctrl+Shift+S) opens an export dialog with the format, quality, lossy compression (smaller GIFs via gifski, lossy WebP), repetitions, output size (10–100 %) and an estimate of the file size; the choice is remembered for the quick "Save". After saving, the path can be copied
- Editor overlays: a progress bar along the top or bottom edge (height, color) that fills up over the animation, and a watermark image (PNG, JPEG, WebP …) in a corner or the center with size and opacity; shown in the preview and burned into the saved GIF/video; the image is kept in the project folder
- Projects as ZIP: the editor exports the open project as `*.framenab.zip` (frames, annotations, effects, watermark); "Open project" and `framenab edit` read it back (also the `project.json` of a recording folder)
- Recovery: while recording, the project is saved every 2 s; a recording interrupted by a crash is marked on the start page and recovered when opened
- Start page: the latest recordings with date, frames, length and size; open them with a click, delete them, or show the recordings folder

### Fixed
- Wayland (XWayland mode): the mouse wheel only scrolled with the pointer near the scrollbar; GTK now gets core X events (`GDK_CORE_DEVICE_EVENTS=1`), so every wheel step arrives

### Changed
- Better contrast (WCAG AA) of buttons and links in the accent color, in light and dark mode
- Tauri 2.12 (Rust crates now match the JavaScript packages; minimum Rust version 1.90)

## [0.3.0] - 2026-11-08

Recording (phase 2) and the frame editor (phase 3): record an area, a window or the whole screen, edit the frames and save them as GIF, MP4 or WebM.

### Added
- Editor transitions: fade into the next frame, fade to a color, or slide (left, right, up, down); inserted after the current or last selected frame with a chosen number of frames and duration; after the last frame the transition leads back to the first
- Editor image tools ("Image"): crop with a frame drawn in the preview (move it, drag its corners, or type the numbers), resize (Lanczos, optionally keeping the aspect ratio, 50 %/75 % presets), rotate by 90° and flip; annotations and the mouse/click effects follow the image; frames are processed in parallel with progress, and undo brings the old frames back
- Editor frame operations: duplicate (Ctrl+D), reverse (selection or everything), yo-yo, move the selected frames by dragging them in the filmstrip, set the duration or change the speed of the selected (or all) frames, remove every n-th frame, and remove similar frames by a pixel threshold (removed frames hand their time to the frame that stays, so the length is unchanged)
- Wayland sessions: FrameNab's windows run on XWayland by default, so the recording frame, its control bar and pins stay on top (also when clicking into other apps) and are placed exactly; capture still uses the portal; Settings → System switches back to native Wayland (after a restart)
- Editor: annotations over frame ranges with the same tools as for screenshots (arrow, line, rectangle, ellipse, pen, highlighter, text, numbered marker, pixelate, blur); new annotations cover the frames selected in the filmstrip; select an annotation to move it, restyle it, put it on other frames or delete it; frames with annotations are marked in the filmstrip; annotations are burned into the saved GIF/video
- Input effects: recordings keep the mouse path, clicks and key presses (`input.json`); in the editor, "Highlight mouse" (a yellow circle follows the cursor), "Clicks" (ripples, colored by button) and "Keys" (captions like `Ctrl + C` at the bottom) are switched on per recording, shown in the preview and burned into the saved GIF/video. On Wayland the mouse position comes from the portal's cursor metadata (FrameNab then draws the cursor into the frames itself); clicks and keys come from the input devices on Linux (needs the `input` group) and from the system on Windows/macOS. Settings → Recording turns recording clicks and keys off
- Editor: new annotations go on all frames by default; a switch limits them to the selected frames, and a selected annotation can be moved to all frames or to the selected frames
- New "Click marker" annotation (K): a translucent circle with a ring, to mark mouse clicks by hand — in the editor and for screenshots
- Editor: the preview fills the available space (small recordings are enlarged), on a plain background instead of a checkerboard
- Editor: "New recording" button; the editor hides during the capture and comes back afterwards (also when the selection is cancelled)
- Wayland recorder shows the frame around the area while recording: the window switches from fullscreen to maximized (which can be transparent on GNOME), finds its position on screen by locating a color marker in a portal screenshot, and draws the frame and the control bar next to the area; if the marker is not found it falls back to the compact bar
- Editor: "Save as…" (Ctrl+Shift+S) picks location and format (GIF, MP4, WebM); the filmstrip only renders visible frames, images are served off the main thread and thumbnails are cached (no more stutter while scrolling); drag over frames to select several (auto-scrolls at the edges); the mouse wheel scrolls the filmstrip
- Editor for recordings (first version): opens automatically after a recording stops; preview with playback (Space, ←/→), a filmstrip of all frames with their durations, select frames (click, Shift/Ctrl+click, Ctrl+A) and delete them (Del) with undo/redo; "Save" writes the result into the save folder with the file name template, in the format chosen in the settings ("Format for recordings": GIF via gifski, MP4/WebM via FFmpeg), with progress and cancel
- "Open project" on the start page and `framenab edit <folder>` open a recording in the editor
- Wayland recorder: GNOME puts black behind transparent fullscreen windows, so before recording the recorder shows the frozen screen image with the frame and the control bar, and from "Record" on the window shrinks to a compact, draggable bar (countdown, timer, pause, stop, result) that leaves the live screen usable
- `framenab record-screen` opens the recorder for the whole screen right away
- Recorder (stage 2b): "Record" opens the selection overlay in record mode (drag an area, click a window on Windows/macOS/X11, "Whole screen", or "App window…" via the system dialog on Wayland); then a borderless, transparent recorder window shows a frame around the area and a control bar (FPS, countdown, mouse cursor, record, pause/resume, stop, timer); everything except the control bar is click-through; the Wayland screen-sharing choice is remembered in the settings; recordings are saved as project folders under the app data folder, with "Open folder" afterwards; "Record" from the tray or a shortcut stops a running recording
- Recording engine (stage 2a): streams the screen via the xdg-desktop-portal ScreenCast portal and PipeWire on Wayland (the screen choice is remembered with a restore token; cursor on/off) and via xcap on Windows, macOS and X11; a common sampler takes the latest frame at the chosen FPS, crops it to the region, excludes paused time and lets unchanged frames last longer; frames are written as QOI by a background writer with a small queue (dropped frames are counted)
- Recording spike: `cargo run --release -p framenab-capture --example record -- <dir> [seconds] [fps] [--cursor] [--region x,y,w,h] [--token-file f]` (60 s at 15 fps in 1080p on Wayland: 0 dropped frames, 126 MB peak memory)

### Fixed
- Editor: after opening another recording, the preview and filmstrip could show the images of the previous one (cached image URLs); a failed change of the effects now shows an error, and "Highlight mouse" is on by default when the recording has mouse data
- Recording time and the timer in the control bar come from the recording clock, so they are exact even when the machine is busy

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
- Recording time no longer counts part of a pause when the machine is busy: pauses are measured when Pause/Resume are clicked, and the end when Stop is clicked
- The countdown number was recorded in the first frames; recording now starts once it is gone from the screen
- Dev mode: windows sometimes opened without their component CSS ("failed to load virtual css module"), e.g. the loupe stuck in the top-left corner; component CSS is now injected through JavaScript
- Pinned screenshots showed a black window: the frontend percent-encodes the protocol path (`pin%2F3`), which the backend did not decode
- Pinning no longer brings back the main window on top of the new pin; the pin gets focus. The main window comes back once the last pin is closed (if it was hidden for the screenshot)
- Wayland: window buttons (minimize, maximize, close) did not react to clicks until the window had been maximized once (tao header bar workaround)
- Wayland: a second screenshot in the same session hung forever

[Unreleased]: https://github.com/Hayababuza/FrameNab/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/Hayababuza/FrameNab/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/Hayababuza/FrameNab/compare/v0.1.0...v0.3.0
[0.1.0]: https://github.com/Hayababuza/FrameNab/releases/tag/v0.1.0
