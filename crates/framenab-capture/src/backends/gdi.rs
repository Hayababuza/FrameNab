//! Windows recording through GDI: copies the recorded rectangle of the desktop with `BitBlt`
//! (like ScreenToGif does by default) and draws the mouse cursor into it.
//!
//! Replaces xcap's DXGI recorder, which
//! - never releases the output duplication when stopped, so the next recording in the same
//!   session fails with "The parameter is incorrect" (`E_INVALIDARG`: "the application is already
//!   duplicating this output"),
//! - records one monitor only, and
//! - leaves the mouse cursor out.
//!
//! Coordinates are physical pixels of the virtual desktop (the app is per-monitor DPI aware), so a
//! rectangle may span several monitors.
#![allow(unsafe_code)] // Win32 GDI calls

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GdiFlush, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, HICON, ICONINFO,
};

use crate::recording::{FrameStream, LatestFrame, SourceFrame};
use crate::{CaptureError, Result};

/// Frame rate when the caller does not say.
const DEFAULT_FPS: u32 = 30;

/// A rectangle of the virtual desktop, grabbed repeatedly into one reusable bitmap.
struct Grabber {
    screen: HDC,
    memory: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *const u8,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl Grabber {
    fn new(x: i32, y: i32, width: u32, height: u32) -> Result<Self> {
        let failed = |what: &str, err: windows::core::Error| CaptureError::Backend(format!("{what}: {err}"));
        // SAFETY: plain GDI calls; every handle created here is released in `Drop`, also on the
        // error paths below.
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return Err(CaptureError::Backend("GetDC failed".into()));
            }
            let memory = CreateCompatibleDC(Some(screen));
            if memory.is_invalid() {
                ReleaseDC(None, screen);
                return Err(CaptureError::Backend("CreateCompatibleDC failed".into()));
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    // Negative height: rows top-down, like an image.
                    biHeight: -(height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bitmap) if !bits.is_null() => bitmap,
                Ok(bitmap) => {
                    let _ = DeleteObject(bitmap.into());
                    let _ = DeleteDC(memory);
                    ReleaseDC(None, screen);
                    return Err(CaptureError::Backend("CreateDIBSection returned no pixels".into()));
                }
                Err(err) => {
                    let _ = DeleteDC(memory);
                    ReleaseDC(None, screen);
                    return Err(failed("CreateDIBSection", err));
                }
            };
            let previous = SelectObject(memory, bitmap.into());
            Ok(Self { screen, memory, bitmap, previous, bits: bits.cast_const().cast(), x, y, width, height })
        }
    }

    /// Copies the rectangle (and the cursor) and returns it as RGBA.
    fn grab(&mut self, cursor: bool) -> Result<Vec<u8>> {
        let (w, h) = (self.width as i32, self.height as i32);
        // SAFETY: the handles are valid for the lifetime of `self`; `bits` points to the DIB
        // section of `width * height` 32-bit pixels, read only after `GdiFlush`.
        let bgra = unsafe {
            BitBlt(self.memory, 0, 0, w, h, Some(self.screen), self.x, self.y, SRCCOPY)
                .map_err(|e| CaptureError::Backend(format!("BitBlt: {e}")))?;
            if cursor {
                draw_cursor(self.memory, self.x, self.y);
            }
            let _ = GdiFlush();
            std::slice::from_raw_parts(self.bits, self.width as usize * self.height as usize * 4)
        };
        let mut rgba = bgra.to_vec();
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            pixel[3] = 255; // BitBlt leaves the alpha byte undefined
        }
        Ok(rgba)
    }
}

impl Drop for Grabber {
    fn drop(&mut self) {
        // SAFETY: releases exactly the handles created in `new`.
        unsafe {
            SelectObject(self.memory, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.memory);
            ReleaseDC(None, self.screen);
        }
    }
}

/// Draws the current mouse cursor into `dc`, whose top-left is at `(x, y)` on the desktop.
///
/// # Safety
/// `dc` must be a valid device context.
unsafe fn draw_cursor(dc: HDC, x: i32, y: i32) {
    let mut info = CURSORINFO { cbSize: size_of::<CURSORINFO>() as u32, ..Default::default() };
    // SAFETY: `info` is a properly sized CURSORINFO; the icon bitmaps GetIconInfo creates are
    // deleted right away.
    unsafe {
        if GetCursorInfo(&mut info).is_err() || info.flags.0 & CURSOR_SHOWING.0 == 0 || info.hCursor.is_invalid() {
            return;
        }
        let icon = HICON(info.hCursor.0);
        let mut icon_info = ICONINFO::default();
        let hotspot = if GetIconInfo(icon, &mut icon_info).is_ok() {
            for bitmap in [icon_info.hbmMask, icon_info.hbmColor] {
                if !bitmap.is_invalid() {
                    let _ = DeleteObject(bitmap.into());
                }
            }
            (icon_info.xHotspot as i32, icon_info.yHotspot as i32)
        } else {
            (0, 0)
        };
        let (cx, cy) = (info.ptScreenPos.x - hotspot.0 - x, info.ptScreenPos.y - hotspot.1 - y);
        let _ = DrawIconEx(dc, cx, cy, icon, 0, 0, 0, None, DI_NORMAL);
    }
}

/// Grabs a rectangle of the desktop `fps` times per second on its own thread.
pub struct GdiStream {
    latest: Arc<LatestFrame>,
    origin: (i32, i32),
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl GdiStream {
    pub fn start(x: i32, y: i32, width: u32, height: u32, fps: u32, cursor: bool) -> Result<Self> {
        let (width, height) = (width.max(1), height.max(1));
        let interval = Duration::from_secs(1) / if fps == 0 { DEFAULT_FPS } else { fps.min(120) };
        let latest = Arc::new(LatestFrame::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = mpsc::channel::<Result<()>>();
        let thread = {
            let (latest, stop) = (Arc::clone(&latest), Arc::clone(&stop));
            thread::Builder::new()
                .name("framenab-gdi-recorder".into())
                .spawn(move || {
                    // GDI handles belong to the thread that uses them: create them here.
                    let mut grabber = match Grabber::new(x, y, width, height) {
                        Ok(g) => g,
                        Err(err) => return drop(started_tx.send(Err(err))),
                    };
                    match grabber.grab(cursor) {
                        Ok(rgba) => latest.publish(SourceFrame { width, height, rgba }),
                        Err(err) => return drop(started_tx.send(Err(err))),
                    }
                    let _ = started_tx.send(Ok(()));
                    let mut failures = 0u32;
                    let mut next = Instant::now() + interval;
                    while !stop.load(Ordering::SeqCst) {
                        let now = Instant::now();
                        if next > now {
                            thread::sleep(next - now);
                        }
                        // Fell behind (slow machine): continue from now instead of catching up.
                        next = (next + interval).max(Instant::now());
                        match grabber.grab(cursor) {
                            Ok(rgba) => {
                                failures = 0;
                                latest.publish(SourceFrame { width, height, rgba });
                            }
                            // Fails briefly while another desktop is shown (UAC prompt, lock
                            // screen); keep the last frame and try again.
                            Err(err) => {
                                failures += 1;
                                if failures == 1 {
                                    tracing::debug!(%err, "screen grab failed");
                                }
                            }
                        }
                    }
                })
                .map_err(|e| CaptureError::Backend(e.to_string()))?
        };
        started_rx.recv().map_err(|e| CaptureError::Backend(e.to_string()))??;
        Ok(Self { latest, origin: (x, y), stop, thread: Some(thread) })
    }
}

impl FrameStream for GdiStream {
    fn latest(&self) -> Arc<LatestFrame> {
        Arc::clone(&self.latest)
    }

    fn origin(&self) -> (i32, i32) {
        self.origin
    }

    fn stop(mut self: Box<Self>) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
