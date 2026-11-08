//! Exporting a recording (a project with many frames) as an animation: GIF via gifski, MP4 and
//! WebM via an installed FFmpeg. Annotations are rendered into every frame they cover.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use framenab_core::{FrameStore, InputEvents, Project, input::effect_annotations};
use image::RgbaImage;
use rgb::FromSlice;

use crate::ExportFormat;

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("The project has no frames")]
    Empty,
    #[error("{0:?} is not an animation format")]
    Unsupported(ExportFormat),
    #[error("FFmpeg is not installed")]
    NoFfmpeg,
    #[error("Export cancelled")]
    Cancelled,
    #[error("Could not read a frame: {0}")]
    Frame(#[from] framenab_core::CoreError),
    #[error("GIF encoder: {0}")]
    Gif(String),
    #[error("FFmpeg: {0}")]
    Ffmpeg(String),
    #[error("Could not write {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Progress and cancellation shared with the UI thread.
#[derive(Debug, Default, Clone)]
pub struct ExportControl {
    /// Frames finished so far.
    pub done: Arc<AtomicUsize>,
    pub cancel: Arc<AtomicBool>,
}

impl ExportControl {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Loads frame `index` and renders the annotations that cover it and the input effects.
fn rendered_frame(
    store: &FrameStore,
    project: &Project,
    input: &InputEvents,
    index: usize,
) -> Result<RgbaImage, ExportError> {
    let base = store.load_frame(&project.frames[index])?;
    let frame = u32::try_from(index).unwrap_or(u32::MAX);
    let render_err = |e: framenab_render::RenderError| ExportError::Gif(e.to_string());
    let mut image = if project.annotations_on(frame).next().is_none() {
        base
    } else {
        framenab_render::render_frame(project, frame, &base).map_err(render_err)?
    };
    if project.effects.any() {
        let fx = effect_annotations(input, project.effects, project.frame_time(index), project.width, project.height);
        framenab_render::render_annotations(&mut image, &fx).map_err(render_err)?;
    }
    Ok(image)
}

/// Exports `project` to `path` in `format`. Blocks until done; call it off the UI thread.
pub fn export_animation(
    store: &FrameStore,
    project: &Project,
    path: &Path,
    format: ExportFormat,
    control: &ExportControl,
) -> Result<(), ExportError> {
    if project.frames.is_empty() {
        return Err(ExportError::Empty);
    }
    let input = if project.effects.any() { store.load_input()? } else { InputEvents::default() };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ExportError::Io { path: parent.to_owned(), source })?;
    }
    let result = match format {
        ExportFormat::Gif => export_gif(store, project, &input, path, control),
        ExportFormat::Mp4 | ExportFormat::Webm => export_video(store, project, &input, path, format, control),
        other => Err(ExportError::Unsupported(other)),
    };
    if result.is_err() {
        // Don't leave a half-written file behind.
        let _ = std::fs::remove_file(path);
    }
    result
}

fn export_gif(
    store: &FrameStore,
    project: &Project,
    input: &InputEvents,
    path: &Path,
    control: &ExportControl,
) -> Result<(), ExportError> {
    let repeat =
        if project.loop_count == 0 { gifski::Repeat::Infinite } else { gifski::Repeat::Finite(project.loop_count) };
    let settings = gifski::Settings { width: None, height: None, quality: 90, fast: false, repeat };
    let (collector, writer) = gifski::new(settings).map_err(|e| ExportError::Gif(e.to_string()))?;

    let file = File::create(path).map_err(|source| ExportError::Io { path: path.to_owned(), source })?;
    let progress = control.clone();
    let encoder = std::thread::spawn(move || {
        let mut reporter = Reporter(progress);
        writer.write(BufWriter::new(file), &mut reporter).map_err(|e| ExportError::Gif(e.to_string()))
    });

    let mut timestamp = 0.0f64;
    let mut feed = || -> Result<(), ExportError> {
        for (i, frame) in project.frames.iter().enumerate() {
            if control.cancelled() {
                return Err(ExportError::Cancelled);
            }
            let image = rendered_frame(store, project, input, i)?;
            let (w, h) = (image.width() as usize, image.height() as usize);
            let pixels = image.as_raw().as_rgba().to_vec();
            collector
                .add_frame_rgba(i, imgref::ImgVec::new(pixels, w, h), timestamp)
                .map_err(|e| ExportError::Gif(e.to_string()))?;
            timestamp += f64::from(frame.delay_ms) / 1000.0;
        }
        Ok(())
    };
    let fed = feed();
    drop(collector); // ends the input; the encoder finishes the file
    let written = encoder.join().map_err(|_| ExportError::Gif("encoder thread panicked".into()))?;
    fed?;
    if control.cancelled() {
        return Err(ExportError::Cancelled);
    }
    written
}

struct Reporter(ExportControl);

impl gifski::progress::ProgressReporter for Reporter {
    fn increase(&mut self) -> bool {
        self.0.done.fetch_add(1, Ordering::Relaxed);
        !self.0.cancelled()
    }

    fn done(&mut self, _msg: &str) {}
}

/// Videos have a constant frame rate: each frame is repeated for as many output frames as its
/// delay covers (carrying the rounding error over to the next frame).
const VIDEO_FPS: u32 = 30;

fn export_video(
    store: &FrameStore,
    project: &Project,
    input: &InputEvents,
    path: &Path,
    format: ExportFormat,
    control: &ExportControl,
) -> Result<(), ExportError> {
    let ffmpeg = crate::find_ffmpeg().ok_or(ExportError::NoFfmpeg)?;
    let codec: &[&str] = match format {
        ExportFormat::Mp4 => &["-c:v", "libx264", "-preset", "medium", "-crf", "20", "-movflags", "+faststart"],
        _ => &["-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "32", "-deadline", "good", "-cpu-used", "4"],
    };
    let size = format!("{}x{}", project.width, project.height);
    let mut child = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &size])
        .args(["-r", &VIDEO_FPS.to_string(), "-i", "-"])
        // Even dimensions are required by yuv420p.
        .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-pix_fmt", "yuv420p"])
        .args(codec)
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ExportError::Ffmpeg(e.to_string()))?;

    let mut stdin = child.stdin.take().expect("piped stdin");
    let frame_ms = 1000.0 / f64::from(VIDEO_FPS);
    let mut carry = 0.0f64;
    let mut write = || -> Result<(), ExportError> {
        for (i, frame) in project.frames.iter().enumerate() {
            if control.cancelled() {
                return Err(ExportError::Cancelled);
            }
            let image = rendered_frame(store, project, input, i)?;
            let exact = f64::from(frame.delay_ms) / frame_ms + carry;
            let repeats = exact.round().max(if i == 0 { 1.0 } else { 0.0 }) as usize;
            carry = exact - repeats as f64;
            for _ in 0..repeats {
                stdin.write_all(image.as_raw()).map_err(|e| ExportError::Ffmpeg(e.to_string()))?;
            }
            control.done.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    };
    let written = write();
    drop(stdin);
    let output = child.wait_with_output().map_err(|e| ExportError::Ffmpeg(e.to_string()))?;
    written?;
    if !output.status.success() {
        return Err(ExportError::Ffmpeg(String::from_utf8_lossy(&output.stderr).trim().to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(dir: &Path, frames: u8) -> (FrameStore, Project) {
        let store = FrameStore::create(dir.join("p.framenab")).unwrap();
        let mut project = Project::new(32, 18);
        for i in 0..frames {
            let image = RgbaImage::from_pixel(32, 18, image::Rgba([i * 60, 100, 200 - i * 40, 255]));
            store.push_frame(&mut project, &image, 100).unwrap();
        }
        (store, project)
    }

    #[test]
    fn exports_a_gif_with_all_frames() {
        let dir = tempfile::tempdir().unwrap();
        let (store, project) = project(dir.path(), 3);
        let out = dir.path().join("out.gif");
        let control = ExportControl::default();
        export_animation(&store, &project, &out, ExportFormat::Gif, &control).unwrap();

        use image::AnimationDecoder;
        let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(File::open(&out).unwrap())).unwrap();
        let frames = decoder.into_frames().collect_frames().unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].buffer().dimensions(), (32, 18));
        assert_eq!(control.done.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn empty_and_cancelled_exports_fail_without_leaving_files() {
        let dir = tempfile::tempdir().unwrap();
        let (store, empty) = project(dir.path(), 0);
        let out = dir.path().join("x.gif");
        assert!(matches!(
            export_animation(&store, &empty, &out, ExportFormat::Gif, &ExportControl::default()),
            Err(ExportError::Empty)
        ));

        let (store, project) = project(dir.path(), 2);
        let control = ExportControl::default();
        control.cancel.store(true, Ordering::Relaxed);
        assert!(export_animation(&store, &project, &out, ExportFormat::Gif, &control).is_err());
        assert!(!out.exists());
    }

    #[test]
    fn exports_mp4_if_ffmpeg_is_installed() {
        if crate::find_ffmpeg().is_none() {
            eprintln!("ffmpeg not installed, skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let (store, project) = project(dir.path(), 3);
        let out = dir.path().join("out.mp4");
        export_animation(&store, &project, &out, ExportFormat::Mp4, &ExportControl::default()).unwrap();
        assert!(std::fs::metadata(&out).unwrap().len() > 100);
    }
}
