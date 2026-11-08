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
        let file = format!("{FRAMES_DIR}/{id}.qoi");
        let path = self.root.join(&file);
        let bytes = qoi::encode_to_vec(image.as_raw(), image.width(), image.height())?;
        fs::write(&path, bytes).at(&path)?;
        project.frames.push(Frame { id, file, delay_ms: delay_ms.max(1), time_ms: None });
        Ok(project.frames.last().expect("just pushed"))
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
