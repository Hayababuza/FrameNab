//! Pinned screenshots: small always-on-top windows showing a rendered selection.
//! (Wayland compositors may ignore "always on top" for normal windows.)

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use image::RgbaImage;
use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::{clipboard, windows};

pub const PREFIX: &str = "pin-";

/// Transparent space around the image (logical pixels) so the resize handles can sit centered on
/// the image border, like on the overlay selection. Keep in sync with `MARGIN` in
/// `src/routes/pin/+page.svelte`.
const MARGIN: f64 = 7.0;

#[derive(Default)]
pub struct PinState {
    images: Mutex<HashMap<u64, Arc<RgbaImage>>>,
    next_id: AtomicU64,
    /// The main window was hidden for the screenshot; show it again when the last pin closes.
    restore: windows::Restore,
}

impl PinState {
    pub fn image(&self, id: u64) -> Option<Arc<RgbaImage>> {
        self.images.lock().expect("pin lock").get(&id).cloned()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinInfo {
    id: u64,
    width: u32,
    height: u32,
    /// The window manager keeps the aspect ratio while resizing (see `lock_aspect`).
    aspect_locked: bool,
}

fn id_from_label(label: &str) -> Option<u64> {
    label.strip_prefix(PREFIX)?.parse().ok()
}

/// Opens a pin window for `image`. `scale` = image pixels per logical pixel (the overlay's scale).
/// `restore`: a window to show again once all pins are closed.
pub fn open(app: &AppHandle, image: RgbaImage, scale: f64, restore: Option<String>) -> tauri::Result<()> {
    let state = app.state::<PinState>();
    state.restore.set(restore);
    let id = state.next_id.fetch_add(1, Ordering::SeqCst) + 1;
    let (w, h) = (f64::from(image.width()) / scale + 2.0 * MARGIN, f64::from(image.height()) / scale + 2.0 * MARGIN);
    state.images.lock().expect("pin lock").insert(id, Arc::new(image));

    let window = WebviewWindowBuilder::new(app, format!("{PREFIX}{id}"), WebviewUrl::App("pin".into()))
        .title("FrameNab – Pin")
        .inner_size(w.max(40.0), h.max(40.0))
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .resizable(true)
        .focused(true)
        .build()?;
    windows::fix_titlebar(&window);
    lock_aspect(&window, w / h);
    windows::focus(&window);
    tracing::info!(id, "pinned screenshot");
    Ok(())
}

/// Keeps the window's aspect ratio while the user resizes it, so the image never gets bars or
/// jumps. On Linux, GTK applies geometry hints during interactive resizes (X11 and Wayland);
/// elsewhere the pin page snaps the size back after resizing.
#[cfg(target_os = "linux")]
fn lock_aspect(window: &tauri::WebviewWindow, aspect: f64) {
    use gtk::gdk::{Geometry, Gravity, WindowHints};
    use gtk::prelude::*;

    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(gtk_window) = target.gtk_window() else { return };
        let geometry = Geometry::new(48, 48, 0, 0, 0, 0, 0, 0, aspect, aspect, Gravity::NorthWest);
        gtk_window.set_geometry_hints(
            None::<&gtk::Widget>,
            Some(&geometry),
            WindowHints::ASPECT | WindowHints::MIN_SIZE,
        );
    });
}

#[cfg(not(target_os = "linux"))]
fn lock_aspect(_window: &tauri::WebviewWindow, _aspect: f64) {}

/// Called when a pin window is destroyed.
pub fn forget(app: &AppHandle, label: &str) {
    let Some(id) = id_from_label(label) else { return };
    let state = app.state::<PinState>();
    let remaining = {
        let mut images = state.images.lock().expect("pin lock");
        images.remove(&id);
        images.len()
    };
    if remaining == 0 {
        state.restore.restore(app);
    }
}

#[tauri::command]
pub fn pin_info(app: AppHandle, window: tauri::WebviewWindow) -> Option<PinInfo> {
    let id = id_from_label(window.label())?;
    let image = app.state::<PinState>().image(id)?;
    Some(PinInfo { id, width: image.width(), height: image.height(), aspect_locked: cfg!(target_os = "linux") })
}

#[tauri::command]
pub async fn copy_pin(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    let id = id_from_label(window.label()).ok_or("not a pin window")?;
    let image = app.state::<PinState>().image(id).ok_or("pin not found")?;
    clipboard::copy_image(&app, (*image).clone()).await
}
