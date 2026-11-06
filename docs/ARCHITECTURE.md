# Architektur

```
┌──────────────────────────── Frontend (SvelteKit, WebView) ─────────────────────────────┐
│  routes/+page (Start/Editor)   routes/overlay   routes/recorder   routes/settings      │
│  lib/annotations (Canvas-Vorschau, Treffertests)   lib/i18n   lib/stores   lib/ipc     │
└───────────────────────────────▲───────────────────────────┬────────────────────────────┘
               Events / Custom-Protocol (Frames)            │ invoke (Commands)
┌───────────────────────────────┴───────────────────────────▼────────────────────────────┐
│ src-tauri: Fenster · Tray · Hotkeys · CLI/Single-Instance · Commands · Logging          │
└──────┬────────────────────┬──────────────────────┬──────────────────────┬──────────────┘
       ▼                    ▼                      ▼                      ▼
 framenab-capture     framenab-core          framenab-render       framenab-export
 CaptureBackend       Project, FrameStore    tiny-skia-Rendering    gifski, APNG, WebP,
 je Plattform         ops, History           der Annotationen       FFmpeg (optional)
```

## Prinzipien

1. **Ein Datenmodell für alles.** Ein Screenshot ist ein `Project` mit einem Frame. Overlay und Editor bearbeiten dieselbe Struktur.
2. **Nicht-destruktiv.** Frames sind unveränderliche QOI-Dateien; Annotationen sind Vektordaten mit Frame-Bereich (`frameStart..=frameEnd`). Pixel werden erst beim Export bzw. beim Kopieren eines Screenshots verrechnet.
3. **Rust ist die Quelle der Wahrheit.** Die Modelltypen werden mit `ts-rs` nach `src/lib/bindings/` generiert. Der Canvas im UI ist nur Vorschau; das Ergebnis rendert `framenab-render`.
4. **Keine Pixeldaten über JSON-IPC.** Frames lädt die UI über ein Custom-Protocol (`framenab://…`, ab Phase 3); Thumbnails werden in Rust erzeugt.
5. **Undo über Snapshots.** Ein `Project` enthält keine Pixel und ist klein, daher genügen Snapshots (`History<T>`) statt eines komplexen Command-Systems.

## Projektordner (`*.framenab`)

```
aufnahme.framenab/
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

Frame-Operationen, die die Reihenfolge ändern (löschen, duplizieren, umkehren, Yo-Yo …), laufen alle über `ops::apply_order`. Dadurch werden die Frame-Bereiche der Annotationen automatisch angepasst.

## Fenster

| Label | Route | Eigenschaften |
|---|---|---|
| `main` | `/` | Hauptfenster; Schließen versteckt es im Tray |
| `overlay` | `/overlay` | Vollbild, randlos, immer im Vordergrund |
| `recorder` | `/recorder` | Aufnahmerahmen |
| `settings` | `/settings` | Einstellungen; Änderungen per Event `settings-changed` an alle Fenster |

Plattformdetails zu Wayland: [wayland.md](wayland.md).
