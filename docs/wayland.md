# FrameNab unter Wayland

Wayland trennt Apps strikt voneinander. Zwei Dinge, die unter Windows, macOS und X11 selbstverständlich sind, gehen dort nur über **xdg-desktop-portal**:

| Funktion | Windows / macOS / X11 | Wayland |
|---|---|---|
| Screenshot | direkt | Portal `Screenshot` |
| Aufnahme | direkt | Portal `ScreenCast` + PipeWire-Stream |
| Globale Tastenkürzel | FrameNab registriert sie selbst | Systemeinstellungen (oder Portal `GlobalShortcuts`, wo verfügbar) |

## Tastenkürzel einrichten

**GNOME:** Einstellungen → Tastatur → Tastenkombinationen anzeigen und anpassen → Eigene Tastenkombinationen → `+`

| Name | Befehl | Taste (Vorschlag) |
|---|---|---|
| FrameNab Screenshot | `framenab gui` | `Druck` |
| FrameNab Aufnahme | `framenab record` | `Umschalt+Druck` |

Belegt GNOME `Druck` bereits mit dem eigenen Screenshot-Tool, dort zuerst die Belegung entfernen.

**KDE Plasma:** Systemeinstellungen → Tastenkürzel → Neu → Befehl oder Skript.

Läuft FrameNab bereits, wird der Befehl an die laufende Instanz weitergereicht (Single-Instance). Sonst startet FrameNab und führt die Aktion direkt aus.

## Freigabe-Dialog

Bei der ersten Aufnahme fragt das System, welcher Bildschirm freigegeben werden soll. FrameNab speichert danach ein *restore token*, damit der Dialog nicht jedes Mal erscheint (geplant ab Phase 1/2).

## Bekannte Probleme

- **Leeres oder weißes Fenster** (manche GPU-Treiber mit WebKitGTK): mit `WEBKIT_DISABLE_DMABUF_RENDERER=1 framenab` starten.
- **Kein Tray-Icon unter GNOME:** Die Erweiterung *AppIndicator and KStatusNotifierItem Support* wird benötigt (bei Ubuntu vorinstalliert).
