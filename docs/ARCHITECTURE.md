# Architecture

```
┌──────────────────────────── Frontend (SvelteKit, WebView) ─────────────────────────────┐
│  routes/+page (start/editor)   routes/overlay   routes/recorder   routes/settings      │
│  lib/annotations (canvas preview, hit tests)   lib/i18n   lib/stores   lib/ipc         │
└───────────────────────────────▲───────────────────────────┬────────────────────────────┘
               events / custom protocol (frames)            │ invoke (commands)
┌───────────────────────────────┴───────────────────────────▼────────────────────────────┐
│ src-tauri: windows · tray · hotkeys · CLI/single instance · commands · logging         │
└──────┬────────────────────┬──────────────────────┬──────────────────────┬──────────────┘
       ▼                    ▼                      ▼                      ▼
 framenab-capture     framenab-core          framenab-render       framenab-export
 CaptureBackend       Project, FrameStore    tiny-skia rendering   gifski, APNG, WebP,
 per platform         ops, History           of annotations        FFmpeg (optional)
```

## Principles

1. **One data model for everything.** A screenshot is a `Project` with one frame. The overlay and the editor edit the same structure.
2. **Non-destructive.** Frames are immutable QOI files; annotations are vector data with a frame range (`frameStart..=frameEnd`). Pixels are only merged on export or when a screenshot is copied.
3. **Rust is the source of truth.** The model types are generated into `src/lib/bindings/` with `ts-rs`. The canvas in the UI is only a preview; `framenab-render` produces the result.
4. **No pixel data over JSON IPC.** The UI loads frames through a custom protocol (`framenab://…`, from phase 3); thumbnails are generated in Rust.
5. **Undo via snapshots.** A `Project` contains no pixels and is small, so snapshots (`History<T>`) are enough; no complex command system is needed.

## Project folder (`*.framenab`)

```
recording.framenab/
├─ project.json
└─ frames/
   ├─ f000001.qoi
   └─ …
```

```jsonc
{
  "version": 1,
  "width": 1280, "height": 720,
  "frames": [{ "id": "f000001", "file": "frames/f000001.qoi", "delayMs": 66 }],
  "annotations": [{
    "id": "a1", "frameStart": 0, "frameEnd": 42,
    "shape": { "type": "arrow", "from": { "x": 100, "y": 80 }, "to": { "x": 300, "y": 200 } },
    "style": { "stroke": { "r": 255, "g": 59, "b": 48, "a": 255 }, "width": 4, "fill": null, "opacity": 1 }
  }],
  "loop": 0,
  "nextFrameId": 1
}
```

Every frame operation that changes the order (delete, duplicate, reverse, yo-yo …) goes through `ops::apply_order`, which adjusts the frame ranges of annotations automatically.

## Windows

| Label | Route | Properties |
|---|---|---|
| `main` | `/` | Main window; closing it hides it to the tray |
| `overlay` | `/overlay` | Fullscreen, borderless, always on top |
| `recorder` | `/recorder` | Recording frame |
| `settings` | `/settings` | Settings; changes reach every window via the `settings-changed` event |

Wayland details: [wayland.md](wayland.md).
