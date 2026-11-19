//! Backend for Windows, macOS and X11 using the `xcap` crate.

use image::RgbaImage;

#[cfg(not(windows))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(windows))]
use std::sync::{Arc, mpsc};
#[cfg(not(windows))]
use std::thread::{self, JoinHandle};
#[cfg(not(windows))]
use std::time::Duration;

#[cfg(not(windows))]
use crate::recording::{LatestFrame, SourceFrame};
use crate::{
    CaptureBackend, CaptureError, CaptureTarget, FrameStream, MonitorInfo, Platform, Result, StreamOptions, WindowInfo,
    bounds,
};

#[derive(Debug, Clone, Copy)]
pub struct XcapBackend(pub Platform);

fn map_err(err: xcap::XCapError) -> CaptureError {
    CaptureError::Backend(err.to_string())
}

impl CaptureBackend for XcapBackend {
    fn platform(&self) -> Platform {
        self.0
    }

    fn monitors(&self) -> Result<Vec<MonitorInfo>> {
        xcap::Monitor::all()
            .map_err(map_err)?
            .iter()
            .map(|m| {
                Ok(MonitorInfo {
                    id: m.id().map_err(map_err)?,
                    name: m.name().unwrap_or_default(),
                    x: m.x().map_err(map_err)?,
                    y: m.y().map_err(map_err)?,
                    width: m.width().map_err(map_err)?,
                    height: m.height().map_err(map_err)?,
                    scale_factor: m.scale_factor().unwrap_or(1.0),
                    is_primary: m.is_primary().unwrap_or(false),
                })
            })
            .collect()
    }

    fn screenshot(&self, target: &CaptureTarget) -> Result<RgbaImage> {
        match target {
            CaptureTarget::AllMonitors => {
                // Stitch all monitors into one image covering the virtual desktop.
                let monitors = xcap::Monitor::all().map_err(map_err)?;
                let infos = self.monitors()?;
                let (min_x, min_y, width, height) = bounds(&infos).ok_or(CaptureError::MonitorNotFound(0))?;
                let mut canvas = RgbaImage::new(width, height);
                for (monitor, info) in monitors.iter().zip(&infos) {
                    let shot = monitor.capture_image().map_err(map_err)?;
                    image::imageops::replace(&mut canvas, &shot, i64::from(info.x - min_x), i64::from(info.y - min_y));
                }
                Ok(canvas)
            }
            CaptureTarget::Monitor(id) => {
                let monitors = xcap::Monitor::all().map_err(map_err)?;
                let monitor =
                    monitors.iter().find(|m| m.id().ok() == Some(*id)).ok_or(CaptureError::MonitorNotFound(*id))?;
                monitor.capture_image().map_err(map_err)
            }
            CaptureTarget::Region { x, y, width, height } => {
                let all = self.screenshot(&CaptureTarget::AllMonitors)?;
                let (min_x, min_y, _, _) = bounds(&self.monitors()?).unwrap_or_default();
                Ok(crate::crop(&all, x - min_x, y - min_y, *width, *height))
            }
            CaptureTarget::Window(_) => Err(CaptureError::NotImplemented(self.0)),
        }
    }

    fn windows(&self) -> Result<Vec<WindowInfo>> {
        let own_pid = std::process::id();
        let mut windows: Vec<(i32, WindowInfo)> = xcap::Window::all()
            .map_err(map_err)?
            .into_iter()
            .filter(|w| !w.is_minimized().unwrap_or(true) && w.pid().ok() != Some(own_pid))
            .filter_map(|w| {
                let info = WindowInfo {
                    x: w.x().ok()?,
                    y: w.y().ok()?,
                    width: w.width().ok()?,
                    height: w.height().ok()?,
                    title: w.title().unwrap_or_default(),
                };
                (info.width > 0 && info.height > 0).then(|| (w.z().unwrap_or(0), info))
            })
            .collect();
        // Higher z = closer to the viewer.
        windows.sort_by_key(|w| std::cmp::Reverse(w.0));
        Ok(windows.into_iter().map(|(_, w)| w).collect())
    }

    #[cfg(windows)]
    fn open_stream(&self, target: &CaptureTarget, options: &StreamOptions) -> Result<Box<dyn FrameStream>> {
        // GDI instead of xcap's DXGI recorder (see `backends::gdi`): grab just the rectangle,
        // which may span monitors.
        let (x, y, width, height) = match target {
            CaptureTarget::Region { x, y, width, height } => (*x, *y, *width, *height),
            other => {
                let monitors = self.monitors()?;
                let monitor = match other {
                    CaptureTarget::Monitor(id) => monitors.iter().find(|m| m.id == *id),
                    _ => monitors.iter().find(|m| m.is_primary).or(monitors.first()),
                }
                .ok_or(CaptureError::MonitorNotFound(0))?;
                (monitor.x, monitor.y, monitor.width, monitor.height)
            }
        };
        let stream = super::gdi::GdiStream::start(x, y, width, height, options.fps, options.cursor)?;
        Ok(Box::new(stream))
    }

    #[cfg(not(windows))]
    fn open_stream(&self, target: &CaptureTarget, _options: &StreamOptions) -> Result<Box<dyn FrameStream>> {
        let monitors = xcap::Monitor::all().map_err(map_err)?;
        let monitor = match target {
            CaptureTarget::Region { x, y, .. } => xcap::Monitor::from_point(*x, *y).map_err(map_err)?,
            CaptureTarget::Monitor(id) => {
                monitors.into_iter().find(|m| m.id().ok() == Some(*id)).ok_or(CaptureError::MonitorNotFound(*id))?
            }
            CaptureTarget::AllMonitors | CaptureTarget::Window(_) => monitors
                .iter()
                .find(|m| m.is_primary().unwrap_or(false))
                .or(monitors.first())
                .cloned()
                .ok_or(CaptureError::MonitorNotFound(0))?,
        };
        Ok(Box::new(XcapStream::start(monitor.id().map_err(map_err)?, origin(&monitor)?)?))
    }
}

#[cfg(not(windows))]
fn origin(monitor: &xcap::Monitor) -> Result<(i32, i32)> {
    Ok((monitor.x().map_err(map_err)?, monitor.y().map_err(map_err)?))
}

/// Recording through xcap's video recorder (ScreenCaptureKit on macOS, XShm on X11; Windows uses
/// `backends::gdi`). The recorder lives entirely on its own thread: on macOS it is not `Send`.
#[cfg(not(windows))]
struct XcapStream {
    latest: Arc<LatestFrame>,
    origin: (i32, i32),
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(not(windows))]
impl XcapStream {
    fn start(monitor_id: u32, origin: (i32, i32)) -> Result<Self> {
        let latest = Arc::new(LatestFrame::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = mpsc::channel::<Result<()>>();
        let thread = {
            let (latest, stop) = (Arc::clone(&latest), Arc::clone(&stop));
            thread::Builder::new()
                .name("framenapper-xcap-recorder".into())
                .spawn(move || {
                    let opened = xcap::Monitor::all()
                        .map_err(map_err)
                        .and_then(|all| {
                            all.into_iter()
                                .find(|m| m.id().ok() == Some(monitor_id))
                                .ok_or(CaptureError::MonitorNotFound(monitor_id))
                        })
                        .and_then(|m| m.video_recorder().map_err(map_err))
                        .and_then(|(recorder, frames)| recorder.start().map_err(map_err).map(|()| (recorder, frames)));
                    let (recorder, frames) = match opened {
                        Ok(r) => {
                            let _ = started_tx.send(Ok(()));
                            r
                        }
                        Err(err) => {
                            let _ = started_tx.send(Err(err));
                            return;
                        }
                    };
                    while !stop.load(Ordering::SeqCst) {
                        match frames.recv_timeout(Duration::from_millis(100)) {
                            Ok(f) => latest.publish(SourceFrame { width: f.width, height: f.height, rgba: f.raw }),
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    }
                    if let Err(err) = recorder.stop() {
                        tracing::debug!(%err, "stopping the recorder failed");
                    }
                })
                .map_err(|e| CaptureError::Backend(e.to_string()))?
        };
        started_rx.recv().map_err(|e| CaptureError::Backend(e.to_string()))??;
        Ok(Self { latest, origin, stop, thread: Some(thread) })
    }
}

#[cfg(not(windows))]
impl FrameStream for XcapStream {
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
