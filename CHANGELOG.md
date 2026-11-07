# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows [SemVer](https://semver.org/).

## [Unreleased]

### Added
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
- Wayland: window buttons (minimize, maximize, close) did not react to clicks until the window had been maximized once (tao header bar workaround)
- Wayland: a second screenshot in the same session hung forever
