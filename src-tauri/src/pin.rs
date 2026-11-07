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

#[derive(Default)]
pub struct PinState {
    images: Mutex<HashMap<u64, Arc<RgbaImage>>>,
    next_id: AtomicU64,
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
}

fn id_from_label(label: &str) -> Option<u64> {
    label.strip_prefix(PREFIX)?.parse().ok()
}

/// Opens a pin window for `image`. `scale` = image pixels per logical pixel (the overlay's scale).
pub fn open(app: &AppHandle, image: RgbaImage, scale: f64) -> tauri::Result<()> {
    let state = app.state::<PinState>();
    let id = state.next_id.fetch_add(1, Ordering::SeqCst) + 1;
    let (w, h) = (f64::from(image.width()) / scale, f64::from(image.height()) / scale);
    state.images.lock().expect("pin lock").insert(id, Arc::new(image));

    let window = WebviewWindowBuilder::new(app, format!("{PREFIX}{id}"), WebviewUrl::App("pin".into()))
        .title("FrameNab – Pin")
        .inner_size(w.max(40.0), h.max(40.0))
        .decorations(false)
        .always_on_top(true)
        .resizable(true)
        .build()?;
    windows::fix_titlebar(&window);
    tracing::info!(id, "pinned screenshot");
    Ok(())
}

/// Called when a pin window is destroyed.
pub fn forget(app: &AppHandle, label: &str) {
    if let Some(id) = id_from_label(label) {
        app.state::<PinState>().images.lock().expect("pin lock").remove(&id);
    }
}

#[tauri::command]
pub fn pin_info(app: AppHandle, window: tauri::WebviewWindow) -> Option<PinInfo> {
    let id = id_from_label(window.label())?;
    let image = app.state::<PinState>().image(id)?;
    Some(PinInfo { id, width: image.width(), height: image.height() })
}

#[tauri::command]
pub async fn copy_pin(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    let id = id_from_label(window.label()).ok_or("not a pin window")?;
    let image = app.state::<PinState>().image(id).ok_or("pin not found")?;
    clipboard::copy_image(&app, (*image).clone()).await
}
