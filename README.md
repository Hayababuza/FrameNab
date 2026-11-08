# FrameNab

**Screenshots and GIF recordings with annotations, for Windows, macOS and Linux.**

FrameNab combines two tools in one app:

- **Screenshot mode:** press a shortcut, select an area, annotate it right away (arrows, text, markers, blur …), then copy or save.
- **Recording mode:** record an area of the screen, then trim it, adjust timing and annotate it in the frame editor, and export as GIF, APNG, WebP or MP4.

Both modes share the same annotation tools. Internally a screenshot is a project with exactly one frame and can be opened in the editor at any time.

> **Status:** v0.3.0 – screenshot mode, recorder and frame editor (phases 1–3) are done; next are more export formats (phase 4). See the [roadmap](docs/PROJECT-PLAN.md#4-features-by-phase).

## Installation

Download the installer for your system from the [releases](https://github.com/Hayababuza/FrameNab/releases):

| System | File |
|---|---|
| Windows 10/11 | `FrameNab_…_x64-setup.exe` (or `.msi`) |
| macOS 11+ (Apple Silicon and Intel) | `FrameNab_…_universal.dmg` – not notarized yet: right-click → *Open* the first time |
| Debian, Ubuntu | `FrameNab_…_amd64.deb` |
| Fedora, openSUSE | `FrameNab-…-1.x86_64.rpm` |
| Other Linux | `FrameNab_…_amd64.AppImage` |

[FFmpeg](https://ffmpeg.org) is optional: with it, recordings can also be saved as MP4, WebM and lossy WebP.

## Tech stack

[Tauri 2](https://tauri.app) · Rust · SvelteKit (Svelte 5) · TypeScript · tiny-skia · gifski

## Development

### Prerequisites

- [Rust](https://rustup.rs) (stable)
- Node.js ≥ 20 and [pnpm](https://pnpm.io)
- System libraries for Tauri: see the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

On Debian/Ubuntu:

```bash
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libssl-dev libxdo-dev \
  libpipewire-0.3-dev libclang-dev libxcb1-dev libxrandr-dev libdbus-1-dev \
  libgbm-dev libegl-dev
```

FFmpeg is optional and only needed for MP4/WebM export and lossy WebP.

### Releases

Raise the version in `package.json`, `src-tauri/tauri.conf.json` and `Cargo.toml` (and the path dependencies), add the release to `CHANGELOG.md`, then push a tag:

```bash
git tag -a v0.4.0 -m "FrameNab 0.4.0" && git push && git push --tags
```

The [release workflow](.github/workflows/release.yml) builds the installers for all three systems and attaches them to a **draft** release; check it on GitHub and publish it.

### Commands

| Command | Purpose |
|---|---|
| `pnpm install` | Install dependencies |
| `pnpm tauri dev` | Run the app in development mode |
| `pnpm tauri build` | Build installers/packages |
| `pnpm test` / `cargo test --workspace` | Tests (frontend / Rust) |
| `pnpm check` / `pnpm lint` | Type checking / linting and format check |
| `cargo clippy --workspace --all-targets` | Rust linting |
| `pnpm bindings` | Regenerate the TypeScript types from the Rust model |

## Command line

```bash
framenab gui        # screenshot overlay
framenab record     # recording
framenab edit <file> # open a recording folder, its project.json or a .framenab.zip
framenab editor     # editor
framenab settings   # settings
framenab --hidden   # start in the tray only
```

If FrameNab is already running, the command is passed on to the running instance.

**Wayland (GNOME, KDE …):** apps are not allowed to register global shortcuts there. Create a custom shortcut in your system settings instead, e.g. `Print` → `framenab gui`. Details: [docs/wayland.md](docs/wayland.md).

## Project structure

```
src/              Frontend (SvelteKit): one route per window, lib/ for logic
src-tauri/        Tauri app: windows, tray, hotkeys, commands (glue code only)
crates/
  framenab-core     Project model, frame store, frame operations, undo
  framenab-capture  Screen capture per platform
  framenab-render   Rendering annotations into frames
  framenab-export   GIF/APNG/WebP/video export
docs/             Plan, architecture, platform notes
```

More in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## License

[MIT](LICENSE). FrameNab is inspired by ScreenToGif and Flameshot but contains no code from either project.
