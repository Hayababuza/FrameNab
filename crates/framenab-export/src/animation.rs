//! Exporting a recording (a project with many frames): GIF via gifski, APNG, animated WebP
//! (lossless built in, lossy via FFmpeg), a PNG sequence as ZIP, and MP4/WebM via an installed
//! FFmpeg. Annotations, effects and overlays are rendered into every frame.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use framenab_core::overlays::{blend_image, watermark_rect};
use framenab_core::{FrameStore, InputEvents, Project, input::effect_annotations};
use image::RgbaImage;
use rgb::FromSlice;

use crate::formats::{ApngWriter, PngSequenceWriter, WebpWriter};
use crate::{ExportFormat, ExportOptions};

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
    #[error("Encoder: {0}")]
    Encode(String),
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

/// What every frame needs besides its own image, prepared once per export.
pub struct Extras {
    /// Size of the exported frames (the project size, scaled).
    size: (u32, u32),
    /// The recorded input, mapped onto the frames (empty without effects).
    input: InputEvents,
    /// The watermark, already scaled, with its top-left corner and opacity.
    watermark: Option<(RgbaImage, i64, i64, f32)>,
}

impl Extras {
    pub fn prepare(store: &FrameStore, project: &Project, options: &ExportOptions) -> Result<Self, ExportError> {
        let input = if project.effects.any() {
            store.load_input()?.transformed(&project.input_transform)
        } else {
            InputEvents::default()
        };
        let watermark = match &project.overlays.watermark {
            Some(wm) => {
                let rect = watermark_rect(project.width, project.height, wm);
                let image = store.load_asset(&wm.file)?;
                let scaled = image::imageops::resize(
                    &image,
                    rect.w as u32,
                    rect.h as u32,
                    image::imageops::FilterType::Lanczos3,
                );
                Some((scaled, rect.x as i64, rect.y as i64, wm.opacity))
            }
            None => None,
        };
        Ok(Self { size: options.output_size(project.width, project.height), input, watermark })
    }
}

/// Loads frame `index` and renders the annotations that cover it, the input effects and the
/// overlays (watermark, progress bar).
pub fn rendered_frame(
    store: &FrameStore,
    project: &Project,
    extras: &Extras,
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
        let fx = effect_annotations(
            &extras.input,
            project.effects,
            project.frame_time(index),
            project.width,
            project.height,
        );
        framenab_render::render_annotations(&mut image, &fx).map_err(render_err)?;
    }
    if let Some((mark, x, y, opacity)) = &extras.watermark {
        blend_image(&mut image, mark, *x, *y, *opacity);
    }
    if let Some(bar) = project.progress_annotation(index) {
        framenab_render::draw(&mut image, &bar).map_err(render_err)?;
    }
    Ok(image)
}

/// [`rendered_frame`] at the export size.
fn output_frame(
    store: &FrameStore,
    project: &Project,
    extras: &Extras,
    index: usize,
) -> Result<RgbaImage, ExportError> {
    let image = rendered_frame(store, project, extras, index)?;
    if image.dimensions() == extras.size {
        return Ok(image);
    }
    Ok(image::imageops::resize(&image, extras.size.0, extras.size.1, image::imageops::FilterType::Lanczos3))
}

/// Exports `project` to `path` in `format`. Blocks until done; call it off the UI thread.
pub fn export_animation(
    store: &FrameStore,
    project: &Project,
    path: &Path,
    format: ExportFormat,
    options: &ExportOptions,
    control: &ExportControl,
) -> Result<(), ExportError> {
    if project.frames.is_empty() {
        return Err(ExportError::Empty);
    }
    let extras = Extras::prepare(store, project, options)?;
    let loop_count = options.loop_count.unwrap_or(project.loop_count);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ExportError::Io { path: parent.to_owned(), source })?;
    }
    let result = match format {
        ExportFormat::Gif => export_gif(store, project, &extras, path, options, loop_count, control),
        ExportFormat::Webp if options.lossy => {
            export_ffmpeg(store, project, &extras, path, format, options, loop_count, control)
        }
        ExportFormat::Apng | ExportFormat::Webp | ExportFormat::PngSequence => {
            export_frames(store, project, &extras, path, format, loop_count, control)
        }
        ExportFormat::Mp4 | ExportFormat::Webm => {
            export_ffmpeg(store, project, &extras, path, format, options, loop_count, control)
        }
    };
    if result.is_err() {
        // Don't leave a half-written file behind.
        let _ = std::fs::remove_file(path);
    }
    result
}

/// APNG, lossless WebP and PNG sequences: one frame after the other into our own writers.
fn export_frames(
    store: &FrameStore,
    project: &Project,
    extras: &Extras,
    path: &Path,
    format: ExportFormat,
    loop_count: u16,
    control: &ExportControl,
) -> Result<(), ExportError> {
    // Only one exists per export, so the size difference of the variants does not matter.
    #[allow(clippy::large_enum_variant)]
    enum Out {
        Apng(ApngWriter),
        Webp(WebpWriter),
        Pngs(PngSequenceWriter),
    }
    let count = project.frames.len();
    let mut out = match format {
        ExportFormat::Apng => Out::Apng(ApngWriter::create(path, extras.size, count as u32, loop_count)?),
        ExportFormat::Webp => Out::Webp(WebpWriter::new(extras.size, loop_count)),
        _ => Out::Pngs(PngSequenceWriter::create(path, count)?),
    };
    for (i, frame) in project.frames.iter().enumerate() {
        if control.cancelled() {
            return Err(ExportError::Cancelled);
        }
        let image = output_frame(store, project, extras, i)?;
        match &mut out {
            Out::Apng(w) => w.add(&image, frame.delay_ms)?,
            Out::Webp(w) => w.add(&image, frame.delay_ms)?,
            Out::Pngs(w) => w.add(&image)?,
        }
        control.done.fetch_add(1, Ordering::Relaxed);
    }
    match out {
        Out::Apng(w) => w.finish(),
        Out::Webp(w) => w.write(path),
        Out::Pngs(w) => w.finish(),
    }
}

fn export_gif(
    store: &FrameStore,
    project: &Project,
    extras: &Extras,
    path: &Path,
    options: &ExportOptions,
    loop_count: u16,
    control: &ExportControl,
) -> Result<(), ExportError> {
    let repeat = if loop_count == 0 { gifski::Repeat::Infinite } else { gifski::Repeat::Finite(loop_count) };
    let quality = options.quality.clamp(1, 100);
    let settings = gifski::Settings { width: None, height: None, quality, fast: false, repeat };
    let (collector, mut writer) = gifski::new(settings).map_err(|e| ExportError::Gif(e.to_string()))?;
    if options.lossy {
        // Not deprecated in substance: gifski's CLI uses it for `--lossy-quality`.
        #[allow(deprecated)]
        writer.set_lossy_quality(quality);
    }

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
            let image = output_frame(store, project, extras, i)?;
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

/// Estimates the size of an export in bytes: exports a few short runs of frames spread over the
/// project to a temporary file and scales the result up (by frames, or by duration for videos).
pub fn estimate_size(
    store: &FrameStore,
    project: &Project,
    format: ExportFormat,
    options: &ExportOptions,
) -> Result<u64, ExportError> {
    const RUNS: usize = 4;
    const RUN: usize = 3;
    let n = project.frames.len();
    if n == 0 {
        return Err(ExportError::Empty);
    }
    let mut picks: Vec<usize> = if n <= RUNS * RUN {
        (0..n).collect()
    } else {
        (0..RUNS).flat_map(|k| (0..RUN).map(move |j| k * (n - RUN) / (RUNS - 1) + j)).collect()
    };
    picks.dedup();
    let mut sample = project.clone();
    framenab_core::ops::apply_order(&mut sample, &picks)?;
    let tmp = std::env::temp_dir().join(format!(
        "framenab-estimate-{}-{}.{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()),
        format.extension()
    ));
    let result = export_animation(store, &sample, &tmp, format, options, &ExportControl::default());
    let size = std::fs::metadata(&tmp).map(|m| m.len());
    let _ = std::fs::remove_file(&tmp);
    result?;
    let size = size.map_err(|source| ExportError::Io { path: tmp.clone(), source })? as f64;
    let factor = if format.requires_ffmpeg() {
        project.duration_ms() as f64 / sample.duration_ms().max(1) as f64
    } else {
        n as f64 / sample.frames.len() as f64
    };
    Ok((size * factor).round() as u64)
}

/// Maps quality 1–100 linearly onto an encoder scale where lower is better (`best`..`worst`).
fn quality_to_crf(quality: u8, best: u32, worst: u32) -> String {
    let q = u32::from(quality.clamp(1, 100));
    (worst - (worst - best) * (q - 1) / 99).to_string()
}

/// MP4, WebM and lossy WebP through an installed FFmpeg.
#[allow(clippy::too_many_arguments)]
fn export_ffmpeg(
    store: &FrameStore,
    project: &Project,
    extras: &Extras,
    path: &Path,
    format: ExportFormat,
    options: &ExportOptions,
    loop_count: u16,
    control: &ExportControl,
) -> Result<(), ExportError> {
    let ffmpeg = crate::find_ffmpeg().ok_or(ExportError::NoFfmpeg)?;
    let codec: Vec<String> = match format {
        ExportFormat::Mp4 => ["-c:v", "libx264", "-preset", "medium", "-movflags", "+faststart", "-crf"]
            .into_iter()
            .map(String::from)
            .chain([quality_to_crf(options.quality, 16, 40)])
            .collect(),
        ExportFormat::Webm => ["-c:v", "libvpx-vp9", "-b:v", "0", "-deadline", "good", "-cpu-used", "4", "-crf"]
            .into_iter()
            .map(String::from)
            .chain([quality_to_crf(options.quality, 15, 50)])
            .collect(),
        _ => ["-c:v", "libwebp_anim", "-lossless", "0", "-q:v"]
            .into_iter()
            .map(String::from)
            .chain([options.quality.clamp(1, 100).to_string(), "-loop".into(), loop_count.to_string()])
            .collect(),
    };
    let size = format!("{}x{}", extras.size.0, extras.size.1);
    // Even dimensions are required by yuv420p in videos.
    let pixels: &[&str] = if format == ExportFormat::Webp {
        &["-pix_fmt", "yuv420p"]
    } else {
        &["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-pix_fmt", "yuv420p"]
    };
    let mut child = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &size])
        .args(["-r", &VIDEO_FPS.to_string(), "-i", "-"])
        .args(pixels)
        .args(&codec)
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
            let image = output_frame(store, project, extras, i)?;
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
        export_animation(&store, &project, &out, ExportFormat::Gif, &ExportOptions::default(), &control).unwrap();

        use image::AnimationDecoder;
        let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(File::open(&out).unwrap())).unwrap();
        let frames = decoder.into_frames().collect_frames().unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].buffer().dimensions(), (32, 18));
        assert_eq!(control.done.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn frames_get_the_watermark_and_a_growing_progress_bar() {
        use framenab_core::overlays::{Anchor, ProgressBar, Watermark};
        let dir = tempfile::tempdir().unwrap();
        let (store, mut project) = project(dir.path(), 2);
        let logo = dir.path().join("logo.png");
        RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255])).save(&logo).unwrap();
        let (file, w, h) = store.import_asset(&logo, "watermark").unwrap();
        project.overlays.watermark = Some(Watermark {
            file,
            image_width: w,
            image_height: h,
            anchor: Anchor::TopLeft,
            size: 0.25,
            opacity: 1.0,
        });
        let green = framenab_core::Rgba { r: 0, g: 255, b: 0, a: 255 };
        project.overlays.progress = Some(ProgressBar { bottom: true, height: 2.0, color: green });

        let extras = Extras::prepare(&store, &project, &ExportOptions::default()).unwrap();
        let first = rendered_frame(&store, &project, &extras, 0).unwrap();
        let last = rendered_frame(&store, &project, &extras, 1).unwrap();
        // Watermark: 8×8 at the 0-pixel margin of an 18-pixel-high frame.
        assert_eq!(first.get_pixel(2, 2).0, [255, 255, 255, 255]);
        // The bar covers half the width after the first of two equal frames, all of it at the end.
        assert_eq!(first.get_pixel(4, 17).0, [0, 255, 0, 255]);
        assert_ne!(first.get_pixel(28, 17).0, [0, 255, 0, 255]);
        assert_eq!(last.get_pixel(28, 17).0, [0, 255, 0, 255]);
    }

    #[test]
    fn exports_apng_webp_and_png_sequences() {
        use image::AnimationDecoder;
        let dir = tempfile::tempdir().unwrap();
        let (store, project) = project(dir.path(), 3);
        let half = ExportOptions { scale: 0.5, ..ExportOptions::default() };

        let apng = dir.path().join("out.png");
        export_animation(&store, &project, &apng, ExportFormat::Apng, &half, &ExportControl::default()).unwrap();
        let decoder = image::codecs::png::PngDecoder::new(std::io::BufReader::new(File::open(&apng).unwrap())).unwrap();
        let frames = decoder.apng().unwrap().into_frames().collect_frames().unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[1].buffer().dimensions(), (16, 9));
        assert_eq!(frames[1].delay().numer_denom_ms(), (100, 1));

        let webp = dir.path().join("out.webp");
        let options = ExportOptions::default();
        export_animation(&store, &project, &webp, ExportFormat::Webp, &options, &ExportControl::default()).unwrap();
        let decoder =
            image::codecs::webp::WebPDecoder::new(std::io::BufReader::new(File::open(&webp).unwrap())).unwrap();
        let frames = decoder.into_frames().collect_frames().unwrap();
        assert_eq!(frames.len(), 3);
        // Lossless: the pixels come back exactly.
        assert_eq!(frames[2].buffer().get_pixel(5, 5).0, [120, 100, 120, 255]);

        let zip_path = dir.path().join("out.zip");
        export_animation(&store, &project, &zip_path, ExportFormat::PngSequence, &options, &ExportControl::default())
            .unwrap();
        let mut zip = zip::ZipArchive::new(File::open(&zip_path).unwrap()).unwrap();
        assert_eq!(zip.len(), 3);
        assert_eq!(zip.by_index(0).unwrap().name(), "frame_0001.png");
    }

    #[test]
    fn estimates_grow_with_the_frames() {
        let dir = tempfile::tempdir().unwrap();
        let (store, project) = project(dir.path(), 3);
        let small = estimate_size(&store, &project, ExportFormat::PngSequence, &ExportOptions::default()).unwrap();
        let mut long = project.clone();
        framenab_core::ops::apply_order(&mut long, &(0..30).map(|i| i % 3).collect::<Vec<_>>()).unwrap();
        let big = estimate_size(&store, &long, ExportFormat::PngSequence, &ExportOptions::default()).unwrap();
        assert!(big > small * 5, "{small} → {big}");
    }

    #[test]
    fn empty_and_cancelled_exports_fail_without_leaving_files() {
        let dir = tempfile::tempdir().unwrap();
        let (store, empty) = project(dir.path(), 0);
        let out = dir.path().join("x.gif");
        assert!(matches!(
            export_animation(
                &store,
                &empty,
                &out,
                ExportFormat::Gif,
                &ExportOptions::default(),
                &ExportControl::default()
            ),
            Err(ExportError::Empty)
        ));

        let (store, project) = project(dir.path(), 2);
        let control = ExportControl::default();
        control.cancel.store(true, Ordering::Relaxed);
        assert!(
            export_animation(&store, &project, &out, ExportFormat::Gif, &ExportOptions::default(), &control).is_err()
        );
        assert!(!out.exists());
    }

    #[test]
    fn exports_lossy_webp_if_ffmpeg_is_installed() {
        if crate::find_ffmpeg().is_none() {
            eprintln!("ffmpeg not installed, skipping");
            return;
        }
        use image::AnimationDecoder;
        let dir = tempfile::tempdir().unwrap();
        let (store, project) = project(dir.path(), 3);
        let out = dir.path().join("out.webp");
        let lossy = ExportOptions { lossy: true, quality: 60, ..ExportOptions::default() };
        export_animation(&store, &project, &out, ExportFormat::Webp, &lossy, &ExportControl::default()).unwrap();
        let decoder =
            image::codecs::webp::WebPDecoder::new(std::io::BufReader::new(File::open(&out).unwrap())).unwrap();
        let frames = decoder.into_frames().collect_frames().unwrap();
        assert!(!frames.is_empty());
        assert_eq!(frames[0].buffer().dimensions(), (32, 18));
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
        export_animation(
            &store,
            &project,
            &out,
            ExportFormat::Mp4,
            &ExportOptions::default(),
            &ExportControl::default(),
        )
        .unwrap();
        assert!(std::fs::metadata(&out).unwrap().len() > 100);
    }
}
