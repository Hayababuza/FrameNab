//! Project folder on disk:
//!
//! ```text
//! <name>.framenab/
//! ├─ project.json
//! └─ frames/
//!    ├─ f000001.qoi
//!    └─ …
//! ```
//!
//! Frames are stored as QOI: lossless and very fast to encode, so recording is not slowed
//! down even at high frame rates.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use image::RgbaImage;

use crate::error::IoContext;
use crate::model::PROJECT_VERSION;
use crate::{CoreError, Frame, Project, Result};

const PROJECT_FILE: &str = "project.json";
const INPUT_FILE: &str = "input.json";
const FRAMES_DIR: &str = "frames";
const ASSETS_DIR: &str = "assets";

/// Whether `file` is a valid asset path (`assets/<name>.png`, no other directories).
#[must_use]
pub fn is_asset_name(file: &str) -> bool {
    file.strip_prefix("assets/").is_some_and(|name| {
        name.ends_with(".png")
            && name.len() > 4
            && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
            && !name.starts_with('.')
    })
}

#[derive(Debug, Clone)]
pub struct FrameStore {
    root: PathBuf,
}

impl FrameStore {
    /// Creates a new project folder (or reuses an existing one).
    pub fn create(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let frames = root.join(FRAMES_DIR);
        fs::create_dir_all(&frames).at(&frames)?;
        Ok(Self { root })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Absolute path of a frame file.
    #[must_use]
    pub fn frame_path(&self, frame: &Frame) -> PathBuf {
        self.root.join(&frame.file)
    }

    /// Stores an image as a new frame and appends it to the project.
    pub fn push_frame<'p>(&self, project: &'p mut Project, image: &RgbaImage, delay_ms: u32) -> Result<&'p Frame> {
        if image.width() != project.width || image.height() != project.height {
            return Err(CoreError::SizeMismatch {
                got_w: image.width(),
                got_h: image.height(),
                want_w: project.width,
                want_h: project.height,
            });
        }
        let id = project.allocate_frame_id();
        let file = self.frame_file_name(&id);
        self.write_image(&file, image)?;
        project.frames.push(Frame { id, file, delay_ms: delay_ms.max(1), time_ms: None });
        Ok(project.frames.last().expect("just pushed"))
    }

    /// Relative file name for a new frame with `id`.
    #[must_use]
    pub fn frame_file_name(&self, id: &str) -> String {
        format!("{FRAMES_DIR}/{id}.qoi")
    }

    /// Writes an image to the frame file `file` (relative to the project folder).
    pub fn write_image(&self, file: &str, image: &RgbaImage) -> Result<()> {
        let path = self.root.join(file);
        let bytes = qoi::encode_to_vec(image.as_raw(), image.width(), image.height())?;
        fs::write(&path, bytes).at(&path)
    }

    /// Loads the pixel data of a frame.
    pub fn load_frame(&self, frame: &Frame) -> Result<RgbaImage> {
        let path = self.frame_path(frame);
        let bytes = fs::read(&path).at(&path)?;
        let (header, data) = qoi::decode_to_vec(&bytes)?;
        let data = if header.channels.as_u8() == 4 {
            data
        } else {
            data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 0xFF]).collect()
        };
        RgbaImage::from_raw(header.width, header.height, data).ok_or(CoreError::SizeMismatch {
            got_w: header.width,
            got_h: header.height,
            want_w: header.width,
            want_h: header.height,
        })
    }

    pub fn save_project(&self, project: &Project) -> Result<()> {
        let path = self.root.join(PROJECT_FILE);
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(project)?).at(&tmp)?;
        // Replace atomically so a crash never leaves a half-written project.json behind.
        fs::rename(&tmp, &path).at(&path)
    }

    pub fn load_project(&self) -> Result<Project> {
        let path = self.root.join(PROJECT_FILE);
        let project: Project = serde_json::from_slice(&fs::read(&path).at(&path)?)?;
        if project.version > PROJECT_VERSION {
            return Err(CoreError::UnsupportedVersion(project.version));
        }
        Ok(project)
    }

    /// Opens the project like [`load_project`](Self::load_project), but first repairs a recording
    /// that was interrupted (crash, power loss): frame files written after the last save of
    /// `project.json` are appended, with durations taken from the file times. Returns the
    /// project and whether it had to be recovered (it is saved again then).
    pub fn recover_project(&self) -> Result<(Project, bool)> {
        let saved = match self.load_project() {
            Ok(project) => Some(project),
            Err(CoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => None,
            Err(CoreError::Json(_)) => None,
            Err(e) => return Err(e),
        };
        if saved.as_ref().is_some_and(|p| !p.recording) {
            return Ok((saved.expect("checked"), false));
        }
        let known = saved.as_ref().map_or(0, |p| p.next_frame_id);
        let mut extra: Vec<(u32, PathBuf, std::time::SystemTime)> = Vec::new();
        let dir = self.root.join(FRAMES_DIR);
        for entry in fs::read_dir(&dir).at(&dir)? {
            let path = entry.at(&dir)?.path();
            let number = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix('f')?.strip_suffix(".qoi")?.parse::<u32>().ok());
            if let Some(number) = number.filter(|&n| n > known) {
                let modified = fs::metadata(&path).and_then(|m| m.modified()).at(&path)?;
                extra.push((number, path, modified));
            }
        }
        extra.sort_by_key(|(n, ..)| *n);

        let header = |path: &Path| -> Result<(u32, u32)> {
            let mut head = [0u8; 14]; // QOI header size
            std::io::Read::read_exact(&mut fs::File::open(path).at(path)?, &mut head).at(path)?;
            let h = qoi::decode_header(head)?;
            Ok((h.width, h.height))
        };
        let mut project = match saved {
            Some(p) => p,
            None => {
                let Some((_, first, _)) = extra.first() else {
                    return Err(CoreError::Io {
                        path: self.root.join(PROJECT_FILE),
                        source: std::io::ErrorKind::NotFound.into(),
                    });
                };
                let (w, h) = header(first)?;
                Project::new(w, h)
            }
        };
        let mut previous: Option<std::time::SystemTime> = None;
        for (number, path, modified) in extra {
            if header(&path).ok() != Some((project.width, project.height)) {
                continue;
            }
            // The previous frame lasted until this one was written.
            if let (Some(prev), Some(last)) = (previous, project.frames.last_mut()) {
                let ms = modified.duration_since(prev).map_or(100, |d| d.as_millis());
                last.delay_ms = u32::try_from(ms.clamp(10, 5000)).unwrap_or(100);
            }
            let id = format!("f{number:06}");
            project.frames.push(Frame { file: self.frame_file_name(&id), id, delay_ms: 100, time_ms: None });
            project.next_frame_id = project.next_frame_id.max(number);
            previous = Some(modified);
        }
        project.recording = false;
        self.save_project(&project)?;
        Ok((project, true))
    }

    /// Copies an image file (PNG, JPEG, WebP …) into the project as a PNG under `assets/`.
    /// Returns its relative file name and size.
    pub fn import_asset(&self, source: &Path, stem: &str) -> Result<(String, u32, u32)> {
        let image = image::open(source).map_err(|e| CoreError::Image(e.to_string()))?.to_rgba8();
        let dir = self.root.join(ASSETS_DIR);
        fs::create_dir_all(&dir).at(&dir)?;
        let mut n = 1;
        let file = loop {
            let file = format!("{ASSETS_DIR}/{stem}-{n}.png");
            if !self.root.join(&file).exists() {
                break file;
            }
            n += 1;
        };
        let path = self.root.join(&file);
        image.save(&path).map_err(|e| CoreError::Image(e.to_string()))?;
        Ok((file, image.width(), image.height()))
    }

    /// Loads an image from `assets/` (see [`import_asset`](Self::import_asset)).
    pub fn load_asset(&self, file: &str) -> Result<RgbaImage> {
        if !is_asset_name(file) {
            return Err(CoreError::Image(format!("invalid asset name {file}")));
        }
        let path = self.root.join(file);
        Ok(image::open(&path).map_err(|e| CoreError::Image(format!("{}: {e}", path.display())))?.to_rgba8())
    }

    /// Saves the recorded input (`input.json`).
    pub fn save_input(&self, input: &crate::InputEvents) -> Result<()> {
        let path = self.root.join(INPUT_FILE);
        fs::write(&path, serde_json::to_vec(input)?).at(&path)
    }

    /// Loads the recorded input; empty if the recording has none.
    pub fn load_input(&self) -> Result<crate::InputEvents> {
        let path = self.root.join(INPUT_FILE);
        match fs::read(&path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(crate::InputEvents::default()),
            Err(e) => Err(e).at(&path),
        }
    }

    /// Deletes frame files that are no longer referenced by the project.
    /// Returns the number of deleted files.
    pub fn collect_garbage(&self, project: &Project) -> Result<usize> {
        let used: HashSet<PathBuf> = project.frames.iter().map(|f| self.frame_path(f)).collect();
        let dir = self.root.join(FRAMES_DIR);
        let mut removed = 0;
        for entry in fs::read_dir(&dir).at(&dir)? {
            let path = entry.at(&dir)?.path();
            if path.extension().is_some_and(|e| e == "qoi") && !used.contains(&path) {
                fs::remove_file(&path).at(&path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn frame_and_project_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FrameStore::create(dir.path().join("test.framenab")).unwrap();
        let mut project = Project::new(3, 2);
        let img = RgbaImage::from_fn(3, 2, |x, y| Rgba([x as u8 * 50, y as u8 * 100, 7, 255]));

        let frame = store.push_frame(&mut project, &img, 66).unwrap().clone();
        assert_eq!(store.load_frame(&frame).unwrap(), img);

        store.save_project(&project).unwrap();
        assert_eq!(store.load_project().unwrap(), project);
    }

    #[test]
    fn recovers_an_interrupted_recording() {
        let dir = tempfile::tempdir().unwrap();
        let store = FrameStore::create(dir.path()).unwrap();
        let mut project = Project::new(2, 2);
        store.push_frame(&mut project, &RgbaImage::new(2, 2), 50).unwrap();
        project.recording = true;
        store.save_project(&project).unwrap();
        // Written after the last save; one has the wrong size.
        let mut later = project.clone();
        store.push_frame(&mut later, &RgbaImage::new(2, 2), 1).unwrap();
        store.write_image("frames/f000003.qoi", &RgbaImage::new(5, 5)).unwrap();
        store.write_image("frames/f000004.qoi", &RgbaImage::new(2, 2)).unwrap();

        let (recovered, changed) = store.recover_project().unwrap();
        assert!(changed && !recovered.recording);
        let ids: Vec<&str> = recovered.frames.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["f000001", "f000002", "f000004"]);
        assert_eq!(recovered.next_frame_id, 4);
        assert_eq!(store.recover_project().unwrap(), (recovered.clone(), false));

        // Without project.json at all, the frames alone make a project.
        fs::remove_file(dir.path().join(PROJECT_FILE)).unwrap();
        let (rebuilt, changed) = store.recover_project().unwrap();
        assert!(changed);
        assert_eq!((rebuilt.width, rebuilt.height, rebuilt.frames.len()), (2, 2, 3));
    }

    #[test]
    fn rejects_wrong_size_and_collects_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let store = FrameStore::create(dir.path()).unwrap();
        let mut project = Project::new(2, 2);
        assert!(store.push_frame(&mut project, &RgbaImage::new(3, 3), 10).is_err());

        store.push_frame(&mut project, &RgbaImage::new(2, 2), 10).unwrap();
        store.push_frame(&mut project, &RgbaImage::new(2, 2), 10).unwrap();
        crate::ops::delete_frames(&mut project, &[0]).unwrap();
        assert_eq!(store.collect_garbage(&project).unwrap(), 1);
    }
}
