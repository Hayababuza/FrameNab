# Changelog

Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionierung nach [SemVer](https://semver.org/lang/de/).

## [Unreleased]

### Hinzugefügt
- Projektgerüst: Tauri 2, SvelteKit (Svelte 5), TypeScript, Cargo-Workspace
- `framenab-core`: Projektmodell, QOI-Frame-Speicher, Frame-Operationen (löschen, duplizieren, umkehren, Yo-Yo, jeden n-ten entfernen, Timing), Undo-Verlauf
- `framenab-render`: Rendering für Linie, Pfeil, Rechteck, Ellipse, Stift, Textmarker, Pixelieren
- `framenab-capture`: `CaptureBackend`-Trait und Plattform-Erkennung (inkl. Wayland)
- `framenab-export`: Exportformate und FFmpeg-Erkennung
- Tray-Menü, Single-Instance mit Befehlsweitergabe (`framenab gui` …), Standard-Tastenkürzel (außer Wayland)
- Startfenster, Einstellungsfenster (Sprache), Platzhalter für Overlay und Rekorder
- Deutsch und Englisch
- CI für Linux, Windows und macOS
