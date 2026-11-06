# FrameNab – Projekt-Prompt & Plan

> **Status:** Freigegeben am 2026-11-06, Phase 0 umgesetzt. Die getroffenen Entscheidungen stehen in §11.

---

## 1. Vision

**FrameNab** ist ein plattformübergreifendes Open-Source-Desktop-Tool, das zwei Werkzeuge vereint:

1. **Schnelle Screenshots mit Annotation** (wie Flameshot): Tastenkürzel → Bildschirm friert ein → Bereich wählen → direkt im Overlay annotieren → kopieren / speichern.
2. **Bildschirmaufnahme als GIF/Video mit Frame-Editor** (wie ScreenToGif): Bereich wählen → aufnehmen → im Editor einzelne Frames bearbeiten, annotieren, kürzen → als GIF / APNG / WebP / MP4 exportieren.

Der Clou der Fusion: **ein gemeinsames Annotationssystem** für beides. Ein Screenshot ist intern einfach ein Projekt mit genau einem Frame. Er kann jederzeit im vollen Editor geöffnet werden, und eine Aufnahme nutzt dieselben Werkzeuge (Pfeil, Text, Blur …).

Es wird **kein Code** aus ScreenToGif (MS-PL) oder Flameshot (GPLv3) übernommen. Beide dienen nur als funktionale Inspiration, Name, Look und Assets sind eigenständig.

---

## 2. Tech-Stack (Empfehlung)

| Bereich | Wahl | Begründung |
|---|---|---|
| App-Framework | **Tauri 2** | Native Fenster auf Windows/macOS/Linux, kleine Binaries (~10 MB statt ~150 MB bei Electron), Tray, Multi-Window, transparente Fenster, eingebauter Bundler (.msi/.dmg/.deb/.AppImage). |
| Backend / Kern | **Rust** (stable, Edition 2024) | Bildverarbeitung und Encoding müssen schnell sein; das beste GIF-Encoding (gifski) ist eine Rust-Bibliothek; plattformspezifische Capture-APIs sind in Rust gut erreichbar. |
| Frontend / UI | **SvelteKit (Svelte 5) + TypeScript + Vite**, statischer Adapter, jedes Fenster ist eine Route | Wenig Boilerplate, sehr performant für Canvas-lastige UIs (Timeline mit hunderten Thumbnails). |
| Zeichnen im UI | **HTML Canvas 2D** (eigene kleine Engine, keine schwere Lib) | Volle Kontrolle; dieselbe Annotations-Datenstruktur wird in Rust für den Export gerendert. |
| Rendering für Export | **tiny-skia** + **cosmic-text** (Rust) | Pixelgenaues, plattformunabhängiges Rendern der Annotationen in die Frames. |
| Paketmanager JS | **pnpm** ⚙️ ANPASSBAR (npm geht auch) | Schnell und platzsparend. |

**Warum nicht …?**
- *Qt/C++ (wie Flameshot):* bewährt, aber deutlich langsamere Entwicklung, und der Frame-Editor wäre mühsamer zu bauen.
- *Electron:* zu groß und zu speicherhungrig für ein Tool, das ständig im Tray läuft.
- *.NET/Avalonia (wie ScreenToGif, nur cross-platform):* möglich, aber die Capture-APIs bräuchten trotzdem pro OS nativen Code, und das GIF-Encoding wäre schwächer.

### Zentrale Rust-Crates (werden in Phase 0 per Spike validiert)
- `xcap`: Screenshots und Monitor-/Fensterliste (Win/macOS/X11, Wayland teilweise)
- `ashpd`: xdg-desktop-portal (Wayland: Screenshot- und ScreenCast-Portal, GlobalShortcuts)
- `pipewire`: Frames aus dem Wayland-ScreenCast-Stream lesen
- `scap` *(Kandidat)*: Videoaufnahme via ScreenCaptureKit (macOS) / Windows.Graphics.Capture (Windows)
- `image`, `qoi`: Bildbearbeitung und schnelles verlustfreies Zwischenspeichern der Frames
- `gifski`: hochwertiges GIF-Encoding
- `png` (APNG), `webp-animator` bzw. `libwebp-sys`: animiertes PNG/WebP
- `arboard`: Zwischenablage (Bild)
- `serde`, `serde_json`, `thiserror`, `anyhow`, `tracing`
- Tauri-Plugins: `global-shortcut`, `dialog`, `fs`, `store` (Settings), `single-instance`, `autostart`, `updater` (später), `cli`

### Externe Abhängigkeit
- **FFmpeg** (optional, als *Sidecar* oder vom System): nur für MP4/WebM-Export. Ohne FFmpeg funktionieren GIF/APNG/WebP/PNG trotzdem. ⚙️ ANPASSBAR: **Entschieden: nur nutzen, wenn installiert.**

---

## 3. Plattform-Support & Capture-Strategie

Capture ist der schwierigste Teil, daher kommt alles hinter ein gemeinsames Trait:

```rust
trait CaptureBackend {
    fn monitors(&self) -> Result<Vec<MonitorInfo>>;
    fn screenshot(&self, target: CaptureTarget) -> Result<RgbaImage>;      // Monitor / Region / Fenster
    fn start_recording(&self, target: CaptureTarget, fps: u32, sink: FrameSink) -> Result<RecordingHandle>;
}
```

| OS | Screenshot | Aufnahme | Globale Hotkeys | Hinweise |
|---|---|---|---|---|
| **Windows 10/11** | xcap (DXGI/GDI) | Windows.Graphics.Capture | Tauri global-shortcut ✅ | Per-Monitor-DPI beachten. |
| **macOS 12.3+** | ScreenCaptureKit | ScreenCaptureKit | Tauri global-shortcut ✅ | Braucht die Berechtigung „Bildschirmaufnahme“; beim ersten Start einen Onboarding-Dialog zeigen. |
| **Linux X11** | xcap (XGetImage/XShm) | xcap im Loop / XShm | Tauri global-shortcut ✅ | Am einfachsten. |
| **Linux Wayland** (GNOME, KDE, wlroots) | **xdg-desktop-portal Screenshot** | **Portal ScreenCast + PipeWire** | **Portal GlobalShortcuts**, wenn verfügbar, sonst CLI-Befehl, den man im System an eine Taste bindet | Siehe unten. |

**Wayland-Strategie (wichtig, dein System läuft damit):**
- Wayland verbietet Apps, den Bildschirm direkt zu lesen. Jede Aufnahme läuft über das Portal; der Nutzer bestätigt einmal, danach wird ein *restore token* gespeichert, damit nicht jedes Mal gefragt wird.
- **Screenshot-Flow wie Flameshot:** Zuerst über das Portal ein Vollbild-Screenshot holen, dann ein eigenes randloses Vollbildfenster mit diesem eingefrorenen Bild öffnen und dort Bereich wählen und annotieren. Das funktioniert überall, auch wo Overlays über anderen Apps eingeschränkt sind.
- **Aufnahme:** Das ScreenCast-Portal liefert einen PipeWire-Stream des ganzen Monitors; auf die gewählte Region wird in Rust zugeschnitten. Der Aufnahmerahmen wird als eigenes Fenster angezeigt, bei Bedarf alternativ nur als Rahmen um die Region.
- **Hotkeys:** `framenab gui`, `framenab screen`, `framenab record` als CLI (wie `flameshot gui`), die Befehle gehen an die laufende Instanz. Die README erklärt, wie man sie in GNOME/KDE einer Taste zuweist.

---

## 4. Funktionsumfang nach Phasen

### Phase 0 – Fundament (Projekt-Setup)
- Tauri 2 + Svelte 5 + TS-Gerüst, Monorepo-Struktur (siehe §5)
- Git-Repo, `.gitignore`, `.editorconfig`, Lizenz, README, CONTRIBUTING
- Linting/Formatting: `rustfmt`, `clippy -D warnings`, ESLint, Prettier, `svelte-check`
- GitHub Actions CI: Build und Tests auf `ubuntu-latest`, `windows-latest`, `macos-latest`
- **Capture-Spikes:** pro OS ein Mini-Test, der 1 Screenshot und 3 Sekunden Aufnahme erzeugt → bestätigt die Crate-Auswahl
- **Abnahme:** `pnpm tauri dev` startet ein leeres Fenster auf allen 3 OS; die CI ist grün.

### Phase 1 – Screenshot-Modus (Flameshot-Teil) → **v0.1**
- Tray-Icon mit Menü: *Screenshot*, *Aufnahme*, *Editor öffnen*, *Einstellungen*, *Beenden*
- Globales Tastenkürzel (Standard `Print` für Screenshot, `Shift+Print` für Aufnahme) ⚙️ ANPASSBAR
- Overlay: eingefrorenes Bild, abgedunkelter Hintergrund, Region per Drag aufziehen, Griffe zum Anpassen, Pixel-Lupe mit Koordinaten und Farbwert, Größenanzeige (B×H)
- Multi-Monitor- und HiDPI-korrekt (unterschiedliche Skalierungen pro Monitor)
- Fenster-Erkennung: Hover hebt das Fenster unter der Maus hervor, ein Klick wählt es (wo das OS das erlaubt)
- **Annotationswerkzeuge** (gemeinsame Engine, siehe §6):
  - Auswahl/Verschieben, Pfeil, Linie, Rechteck, Ellipse (gefüllt/umrandet), Freihand-Stift, Textmarker
  - Text (Schriftart, Größe, Farbe, Hintergrund)
  - Nummerierte Marker (Zähler 1, 2, 3 …)
  - Pixelieren und Weichzeichnen (Bereich)
  - Farbwähler, Strichstärke, Undo/Redo (Strg+Z / Strg+Y)
- Aktionen: **In die Zwischenablage kopieren** (Enter / Strg+C), **Speichern** (Strg+S, Dateiname-Template z. B. `FrameNab_{yyyy-MM-dd_HH-mm-ss}.png`), **Im Editor öffnen**, Pin (als schwebendes Fenster immer im Vordergrund halten), Abbrechen (Esc)
- Ausgabe: PNG, JPG, WebP
- **Abnahme:** Screenshot einer Region mit Pfeil und Text landet in unter 1 s nach Enter in der Zwischenablage, auf allen 3 OS und unter GNOME-Wayland.

### Phase 2 – Aufnahme (ScreenToGif-Recorder) → **v0.2**
- Rekorder-Fenster: verschiebbarer und skalierbarer Rahmen um die Aufnahmeregion, Größenfelder (B×H), Presets (z. B. 640×480, 1280×720)
- Steuerung: Aufnahme / Pause / Stopp, FPS (1–60, Standard 15) ⚙️ ANPASSBAR, Countdown (0–5 s)
- Optional: Mauszeiger aufnehmen, Klicks hervorheben (Kreis an der Klickposition, wo das OS Klick-Events liefert)
- Frames werden als **QOI auf die Platte** gestreamt (temporärer Projektordner), nicht im RAM gehalten, damit auch lange Aufnahmen stabil laufen
- Frame-Delays aus den echten Zeitstempeln (variable Framerate)
- Nach dem Stopp → automatisch in den Editor
- **Abnahme:** Eine 60-s-Aufnahme bei 15 fps in 1080p läuft ohne Framedrops über 5 % und mit weniger als 500 MB RAM.

### Phase 3 – Frame-Editor (ScreenToGif-Editor) → **v0.3**
- Layout: oben die Werkzeugleiste (Ribbon/Tabs), in der Mitte die Vorschau mit Zoom, unten eine **Timeline mit Thumbnails** (virtualisiert, also flüssig auch bei 2000+ Frames), rechts das Eigenschaften-Panel
- Wiedergabe: Play/Pause, Loop, Frame vor/zurück, Gesamtdauer
- **Frame-Operationen:** auswählen (einzeln, Bereich mit Shift, mehrere mit Strg), löschen, duplizieren, Reihenfolge umkehren, Yo-Yo (vorwärts+rückwärts), jedes n-te Frame entfernen, doppelte bzw. fast identische Frames entfernen (Schwellwert), verschieben per Drag
- **Timing:** Delay pro Frame bzw. Auswahl setzen, vergrößern/verkleinern um %, Geschwindigkeit skalieren
- **Bild:** zuschneiden, skalieren (Lanczos), drehen/spiegeln, Rahmen, Hintergrund/Padding
- **Annotationen über Frame-Bereiche:** jede Annotation hat `frameStart..frameEnd`, so lassen sich z. B. Untertitel oder Pfeile nur für bestimmte Frames einblenden; Werkzeuge wie in Phase 1, dazu Wasserzeichen (Bild) und Fortschrittsbalken
- **Übergänge:** Fade und Slide zwischen Frames einfügen
- Projekt speichern und laden (`.framenab`-Ordner bzw. ZIP, siehe §6), Wiederherstellung nach einem Absturz
- Undo/Redo für alle Operationen (Command-Pattern)

### Phase 4 – Export → **v0.4**
- **GIF** über gifski: Qualität (1–100), Loop-Anzahl, max. Farben, Dithering, optionales Lossy
- **APNG**, **animiertes WebP** (verlustfrei/verlustbehaftet)
- **MP4 (H.264) / WebM (VP9)** über FFmpeg, wenn verfügbar
- **PNG-Sequenz** (ZIP oder Ordner)
- Export-Dialog mit **Größenschätzung** und Fortschrittsbalken, abbrechbar, läuft im Hintergrund-Thread
- Nach dem Export: Datei im Dateimanager zeigen oder in die Zwischenablage kopieren (Dateipfad)

### Phase 5 – Feinschliff → **v1.0**
- Einstellungsfenster: Hotkeys, Speicherort, Dateiname-Template, Standard-FPS, Theme (Hell/Dunkel/System), Sprache, Autostart
- **i18n:** Deutsch und Englisch von Anfang an (Strings nie hartcodiert) ⚙️ ANPASSBAR
- Installer und Pakete: Windows `.msi` + NSIS `.exe`, macOS `.dmg` (Universal: Intel + Apple Silicon), Linux `.deb`, `.rpm`, `.AppImage`; Flatpak später
- Release-Workflow in GitHub Actions (bei einem Tag `v*` werden alle Pakete gebaut und als Draft-Release hochgeladen)
- Auto-Updater (Tauri updater, signiert) ⚙️ ANPASSBAR
- Barrierefreiheit: Tastaturbedienung im Editor, ausreichende Kontraste

### Nach v1.0 (Ziel: möglichst alles aus ScreenToGif und Flameshot umsetzen)
- Webcam-Aufnahme und Zeichenbrett-Modus (wie bei ScreenToGif)
- Audio bei Videoexport
- Upload (Imgur, eigener Server), OCR, Fenster-Scroll-Screenshots
- Plugin-System

---

## 5. Projektstruktur

```
FrameNab/
├─ .github/workflows/        ci.yml, release.yml
├─ docs/                     PROJEKT-PROMPT.md (dieses Dokument), ARCHITECTURE.md, wayland.md
├─ src/                      # Frontend (Svelte 5 + TS)
│  ├─ lib/
│  │  ├─ annotations/        Datentypen, Canvas-Renderer, Werkzeuge (Pfeil, Text, …)
│  │  ├─ components/         UI-Bausteine (Toolbar, ColorPicker, Timeline, …)
│  │  ├─ ipc/                typsichere Wrapper um Tauri-Commands und -Events
│  │  ├─ i18n/               de.json, en.json
│  │  └─ stores/             Svelte-Stores (Settings, Editor-State, Undo-Stack)
│  ├─ routes/                ein SvelteKit-Route pro Fenster:
│  │  ├─ +page.svelte        Hauptfenster (Start, später Editor)
│  │  ├─ overlay/            Screenshot-Overlay
│  │  ├─ recorder/           Aufnahmerahmen + Steuerung
│  │  └─ settings/
│  └─ app.css
├─ src-tauri/                # Rust-App (dünne Schicht: Fenster, Tray, Commands)
│  ├─ src/
│  │  ├─ main.rs, lib.rs
│  │  ├─ commands/           #[tauri::command]-Funktionen
│  │  ├─ tray.rs, hotkeys.rs, cli.rs, windows.rs
│  ├─ icons/, capabilities/, tauri.conf.json, Cargo.toml
├─ crates/                   # Kernlogik, unabhängig von Tauri und dadurch testbar
│  ├─ framenab-capture/      CaptureBackend-Trait + Backends: windows, macos, x11, wayland
│  ├─ framenab-core/         Projektmodell, Frame-Store (QOI), Undo-Commands, Frame-Operationen
│  ├─ framenab-render/       Annotationen rendern (tiny-skia, cosmic-text), Blur/Pixelate
│  └─ framenab-export/       GIF (gifski), APNG, WebP, FFmpeg-Anbindung
├─ Cargo.toml                (Workspace)
├─ package.json, pnpm-lock.yaml, vite.config.ts, tsconfig.json, svelte.config.js
├─ .editorconfig, .gitignore, .prettierrc, eslint.config.js, rustfmt.toml
├─ LICENSE, README.md, CONTRIBUTING.md, CHANGELOG.md
```

**Grundsatz:** `src-tauri` enthält nur Glue-Code. Die Logik liegt in `crates/*` und hat Unit-Tests, die ohne GUI laufen.

---

## 6. Datenmodell (Kern)

```jsonc
// project.json in <projekt>.framenab/
{
  "version": 1,
  "width": 1280, "height": 720,
  "frames": [
    { "id": "f0001", "file": "frames/f0001.qoi", "delayMs": 66 }
  ],
  "annotations": [
    {
      "id": "a1", "type": "arrow",          // arrow|line|rect|ellipse|pen|highlighter|text|counter|blur|pixelate|image|progressbar
      "frameStart": 0, "frameEnd": 42,     // Screenshot: 0..0
      "geometry": { "from": [100, 80], "to": [300, 200] },
      "style": { "stroke": "#FF3B30", "width": 4, "fill": null, "opacity": 1 }
    }
  ],
  "loop": 0
}
```

- Annotationen bleiben **nicht-destruktiv**, bis exportiert wird.
- Dasselbe JSON-Schema wird in TypeScript (UI-Vorschau) und Rust (Export-Rendering) genutzt. Die TS-Typen werden mit `ts-rs` oder `specta` **aus Rust generiert**, damit beide Seiten nie auseinanderlaufen.
- Visuelle Regressionstests stellen sicher, dass der Canvas-Renderer (UI) und tiny-skia (Export) praktisch gleich aussehen.
- Frames gehen nie als Base64 über IPC: Die UI lädt sie über ein **Custom-Protocol** (`framenab://frame/<projekt>/<id>?thumb=1`), das Rust bedient.

---

## 7. Qualität & Konventionen

- **Tests:** Rust-Unit-Tests in allen `crates/*`, Vitest für die Frontend-Logik (Annotations-Geometrie, Undo-Stack), ein Playwright-Smoke-Test, der das Editorfenster mit einem Testprojekt lädt
- **CI:** jeder Push und PR führt fmt-check, clippy, svelte-check, ESLint, die Tests und einen Build auf allen 3 OS aus
- **Commits:** Conventional Commits (`feat:`, `fix:`, `chore:` …) ⚙️ ANPASSBAR
- **Branches:** `main` (stabil), Feature-Branches → PR
- **Logging:** `tracing` mit Logdatei im App-Datenordner, umschaltbar auf Debug
- **Fehler:** Nutzer sehen immer verständliche Meldungen (übersetzt), keine Panics im UI-Pfad

---

## 8. Lizenz

**MIT** (entschieden).

---

## 9. Voraussetzungen auf deinem Rechner (Ubuntu 26.04, GNOME/Wayland)

Fehlt aktuell: **Rust, pnpm, FFmpeg (optional)**. Wird nach deinem OK installiert bzw. dir als Befehl gezeigt:

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Tauri-/Linux-Abhängigkeiten
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libssl-dev libxdo-dev \
  libpipewire-0.3-dev libclang-dev libxcb1-dev libxrandr-dev libdbus-1-dev
# pnpm
corepack enable && corepack prepare pnpm@latest --activate
# optional
sudo apt install ffmpeg
```

> `sudo`-Befehle führe ich nicht selbst aus. Du bekommst sie zum Ausführen.

---

## 10. Was nach deinem OK konkret passiert

1. Voraussetzungen prüfen bzw. installieren (Rust, pnpm; `sudo`-Teil übernimmst du)
2. Tauri-2-Projekt mit Svelte 5 + TS erzeugen und auf die Struktur aus §5 umbauen (Cargo-Workspace mit `crates/*`)
3. Tooling: rustfmt, clippy, ESLint, Prettier, svelte-check, Vitest, `.editorconfig`
4. Leere, kompilierende Grundgerüste für alle Crates, inklusive `CaptureBackend`-Trait und Projektmodell mit ersten Tests
5. Tray-Icon und ein erstes Overlay-Fenster als Platzhalter
6. README, LICENSE, CONTRIBUTING, CHANGELOG, `docs/ARCHITECTURE.md`; dieses Dokument wandert nach `docs/`
7. GitHub-Actions-CI-Workflow
8. `git init` auf `main`, sinnvolle erste Commits (kein Push und kein Remote, außer du willst eins)
9. Prüfen: `pnpm tauri dev` startet lokal bei dir

**Danach** geht es phasenweise weiter (Phase 1 → v0.1 usw.), jeweils mit kurzer Rückmeldung von dir.

---

## 11. Entscheidungen (2026-11-06)

- **Frontend:** Svelte (SvelteKit, Svelte 5)
- **Lizenz:** MIT
- **FFmpeg:** nicht mitliefern; nutzen, wenn installiert
- **Sprachen:** Deutsch + Englisch zum Start
- **Git:** nur lokal, kein Remote
- **Funktionsumfang:** Wenn möglich soll *alles* aus ScreenToGif und Flameshot machbar sein. Der Abschnitt „Nach v1.0“ ist deshalb Teil der Roadmap und kein optionaler Ausblick.
