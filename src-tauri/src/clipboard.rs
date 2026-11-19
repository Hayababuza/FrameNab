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
        // Hand the data to a clipboard manager (if any) so it survives FrameNapper quitting.
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

/// Puts a file on the clipboard (like "Copy" in the file manager), so a saved GIF or video can be
/// pasted into chats, mails and documents with Ctrl+V.
#[cfg(target_os = "linux")]
pub async fn copy_file(app: &AppHandle, path: std::path::PathBuf) -> Result<(), String> {
    use gtk::{TargetEntry, TargetFlags};

    let uri = gtk::glib::filename_to_uri(&path, None).map_err(|e| e.to_string())?.to_string();
    let text = path.display().to_string();
    let (tx, rx) = tauri::async_runtime::channel(1);
    app.run_on_main_thread(move || {
        // GNOME/Nautilus, then the generic file list, then the path as text.
        let targets = [
            TargetEntry::new("x-special/gnome-copied-files", TargetFlags::empty(), 0),
            TargetEntry::new("text/uri-list", TargetFlags::empty(), 1),
            TargetEntry::new("UTF8_STRING", TargetFlags::empty(), 2),
            TargetEntry::new("text/plain;charset=utf-8", TargetFlags::empty(), 2),
        ];
        let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
        let ok = clipboard.set_with_data(&targets, move |_, data, info| match info {
            0 => data.set(&data.target(), 8, format!("copy\n{uri}").as_bytes()),
            1 => {
                data.set_uris(&[uri.as_str()]);
            }
            _ => {
                data.set_text(&text);
            }
        });
        let _ = tx.try_send(ok);
    })
    .map_err(|e| e.to_string())?;
    let mut rx = rx;
    match rx.recv().await {
        Some(true) => Ok(()),
        Some(false) => Err("the clipboard did not accept the file".into()),
        None => Err("clipboard task was dropped".into()),
    }
}

#[cfg(not(target_os = "linux"))]
pub async fn copy_file(_app: &AppHandle, path: std::path::PathBuf) -> Result<(), String> {
    arboard::Clipboard::new().and_then(|mut c| c.set().file_list(&[path])).map_err(|e| e.to_string())
}
