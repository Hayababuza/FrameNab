# FrameNab on Wayland

Wayland isolates apps from each other. Two things that are taken for granted on Windows, macOS and X11 only work through **xdg-desktop-portal** there:

| Feature | Windows / macOS / X11 | Wayland |
|---|---|---|
| Screenshot | direct | `Screenshot` portal |
| Recording | direct | `ScreenCast` portal + PipeWire stream |
| Global shortcuts | FrameNab registers them itself | system settings (or the `GlobalShortcuts` portal where available) |

## Setting up shortcuts

**GNOME:** Settings → Keyboard → View and Customize Shortcuts → Custom Shortcuts → `+`

| Name | Command | Key (suggestion) |
|---|---|---|
| FrameNab Screenshot | `framenab gui` | `Print` |
| FrameNab Recording | `framenab record` | `Shift+Print` |

If GNOME already uses `Print` for its own screenshot tool, remove that binding first.

**KDE Plasma:** System Settings → Shortcuts → New → Command or Script.

If FrameNab is already running, the command is forwarded to the running instance (single instance). Otherwise FrameNab starts and runs the action right away.

## Screen sharing dialog

On the first capture the system asks which screen to share. FrameNab then stores a *restore token* so the dialog does not appear every time (planned for phase 1/2).

## Known issues

- **Blank or white window** (some GPU drivers with WebKitGTK): start with `WEBKIT_DISABLE_DMABUF_RENDERER=1 framenab`.
- **No tray icon on GNOME:** requires the *AppIndicator and KStatusNotifierItem Support* extension (preinstalled on Ubuntu).
