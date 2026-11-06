# FrameNab

**Screenshots und GIF-Aufnahmen mit Annotation, für Windows, macOS und Linux.**

FrameNab vereint zwei Werkzeuge in einer App:

- **Screenshot-Modus:** Tastenkürzel drücken, Bereich wählen, direkt annotieren (Pfeile, Text, Marker, Blur …), dann kopieren oder speichern.
- **Aufnahme-Modus:** Bildschirmbereich aufnehmen, im Frame-Editor schneiden, Timing anpassen und annotieren, dann als GIF, APNG, WebP oder MP4 exportieren.

Beide Modi nutzen dieselben Annotationswerkzeuge. Ein Screenshot ist intern ein Projekt mit genau einem Frame und lässt sich jederzeit im Editor öffnen.

> **Status:** Phase 0 (Fundament). Siehe [Roadmap](docs/PROJEKT-PROMPT.md#4-funktionsumfang-nach-phasen).

## Tech-Stack

[Tauri 2](https://tauri.app) · Rust · SvelteKit (Svelte 5) · TypeScript · tiny-skia · gifski

## Entwicklung

### Voraussetzungen

- [Rust](https://rustup.rs) (stable)
- Node.js ≥ 20 und [pnpm](https://pnpm.io)
- Systembibliotheken für Tauri: siehe [Tauri-Voraussetzungen](https://v2.tauri.app/start/prerequisites/)

Unter Debian/Ubuntu:

```bash
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libssl-dev libxdo-dev \
  libpipewire-0.3-dev libclang-dev libxcb1-dev libxrandr-dev libdbus-1-dev \
  libgbm-dev libegl-dev
```

FFmpeg ist optional und wird nur für den MP4/WebM-Export gebraucht.

### Befehle

| Befehl | Zweck |
|---|---|
| `pnpm install` | Abhängigkeiten installieren |
| `pnpm tauri dev` | App im Entwicklungsmodus starten |
| `pnpm tauri build` | Installer/Pakete bauen |
| `pnpm test` / `cargo test --workspace` | Tests (Frontend / Rust) |
| `pnpm check` / `pnpm lint` | Typprüfung / Linting + Formatierung prüfen |
| `cargo clippy --workspace --all-targets` | Rust-Linting |
| `pnpm bindings` | TypeScript-Typen aus dem Rust-Modell neu erzeugen |

## Bedienung per Kommandozeile

```bash
framenab gui        # Screenshot-Overlay
framenab record     # Aufnahme
framenab editor     # Editor
framenab settings   # Einstellungen
framenab --hidden   # nur im Tray starten
```

Läuft FrameNab bereits, wird der Befehl an die laufende Instanz weitergegeben.

**Wayland (GNOME, KDE …):** Apps dürfen dort keine globalen Tastenkürzel registrieren. Lege in den Systemeinstellungen ein eigenes Tastenkürzel an, z. B. `Druck` → `framenab gui`. Details: [docs/wayland.md](docs/wayland.md).

## Projektstruktur

```
src/              Frontend (SvelteKit): eine Route pro Fenster, lib/ für Logik
src-tauri/        Tauri-App: Fenster, Tray, Hotkeys, Commands (nur Glue-Code)
crates/
  framenab-core     Projektmodell, Frame-Speicher, Frame-Operationen, Undo
  framenab-capture  Bildschirmaufnahme je Plattform
  framenab-render   Annotationen in Frames rendern
  framenab-export   GIF/APNG/WebP/Video-Export
docs/             Plan, Architektur, Plattformhinweise
```

Mehr dazu in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Lizenz

[MIT](LICENSE). FrameNab ist von ScreenToGif und Flameshot inspiriert, enthält aber keinen Code aus diesen Projekten.
