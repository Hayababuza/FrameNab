//! Backend for Windows, macOS and X11 using the `xcap` crate.

use image::RgbaImage;

use crate::{
    CaptureBackend, CaptureError, CaptureTarget, FrameSink, MonitorInfo, Platform, RecordingHandle, Result, WindowInfo,
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

    fn start_recording(&self, _: &CaptureTarget, _: u32, _: Box<dyn FrameSink>) -> Result<Box<dyn RecordingHandle>> {
        Err(CaptureError::NotImplemented(self.0))
    }
}
