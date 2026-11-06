//! Copying images to the system clipboard.
//!
//! On Linux we go through GTK, which the webview already uses: it speaks the native Wayland
//! clipboard protocol (GNOME does not support the wlr data-control protocol other crates rely on)
//! and works on X11 as well. Elsewhere `arboard` is used.

use image::RgbaImage;
use tauri::AppHandle;

#[cfg(target_os = "linux")]
pub async fn copy_image(app: &AppHandle, image: RgbaImage) -> Result<(), String> {
    use gtk::gdk_pixbuf::{Colorspace, Pixbuf};

    let (tx, rx) = tauri::async_runtime::channel(1);
    // GTK may only be used from the main thread.
    app.run_on_main_thread(move || {
        let (w, h) = image.dimensions();
        let bytes = gtk::glib::Bytes::from_owned(image.into_raw());
        let pixbuf = Pixbuf::from_bytes(&bytes, Colorspace::Rgb, true, 8, w as i32, h as i32, (w * 4) as i32);
        let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
        clipboard.set_image(&pixbuf);
        // Hand the data to a clipboard manager (if any) so it survives FrameNab quitting.
        clipboard.store();
        let _ = tx.try_send(());
    })
    .map_err(|e| e.to_string())?;
    let mut rx = rx;
    rx.recv().await.ok_or_else(|| "clipboard task was dropped".to_owned())
}

#[cfg(not(target_os = "linux"))]
pub async fn copy_image(_app: &AppHandle, image: RgbaImage) -> Result<(), String> {
    let (w, h) = image.dimensions();
    let data = arboard::ImageData { width: w as usize, height: h as usize, bytes: image.into_raw().into() };
    arboard::Clipboard::new().and_then(|mut c| c.set_image(data)).map_err(|e| e.to_string())
}
