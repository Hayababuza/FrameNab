//! A project as a single ZIP file (`*.framenapper.zip`), to pass it on or keep it: `project.json`,
//! `input.json` (if any) and the frame files the project uses.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::IoContext;
use crate::{CoreError, FrameStore, Project, Result};

/// File name ending of project archives.
pub const EXTENSION: &str = "framenapper.zip";

fn zip_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::Archive(e.to_string())
}

/// Writes `project` (as stored in `store`) to the archive `dest`.
pub fn export_zip(store: &FrameStore, project: &Project, dest: &Path) -> Result<()> {
    let tmp = dest.with_extension("zip.tmp");
    let file = fs::File::create(&tmp).at(&tmp)?;
    let mut zip = ZipWriter::new(file);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    // QOI is already compressed; storing is much faster and hardly bigger.
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    let mut project = project.clone();
    project.recording = false;
    zip.start_file("project.json", deflated).map_err(zip_err)?;
    zip.write_all(&serde_json::to_vec_pretty(&project)?).map_err(zip_err)?;

    let input = store.load_input()?;
    if !input.is_empty() {
        zip.start_file("input.json", deflated).map_err(zip_err)?;
        zip.write_all(&serde_json::to_vec(&input)?).map_err(zip_err)?;
    }

    if let Some(wm) = &project.overlays.watermark {
        let path = store.root().join(&wm.file);
        zip.start_file(wm.file.as_str(), stored).map_err(zip_err)?;
        zip.write_all(&fs::read(&path).at(&path)?).map_err(zip_err)?;
    }

    let mut written = HashSet::new();
    for frame in &project.frames {
        if !written.insert(frame.file.as_str()) {
            continue;
        }
        let path = store.frame_path(frame);
        zip.start_file(frame.file.as_str(), stored).map_err(zip_err)?;
        zip.write_all(&fs::read(&path).at(&path)?).map_err(zip_err)?;
    }
    zip.finish().map_err(zip_err)?;
    fs::rename(&tmp, dest).at(dest)
}

/// Whether an archive entry may be extracted: only the known files, no paths outside.
fn allowed(name: &str) -> bool {
    match name {
        "project.json" | "input.json" => true,
        _ if crate::store::is_asset_name(name) => true,
        _ => name.strip_prefix("frames/").is_some_and(|file| {
            file.ends_with(".qoi") && !file.is_empty() && file.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
        }),
    }
}

/// Extracts the archive `src` into a new project folder inside `dest_dir` and returns it.
pub fn import_zip(src: &Path, dest_dir: &Path) -> Result<PathBuf> {
    let file = fs::File::open(src).at(src)?;
    let mut zip = ZipArchive::new(file).map_err(zip_err)?;
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("project");
    let base = name.strip_suffix(".zip").unwrap_or(name);
    // `.framenab.zip`: archives from before the app was renamed.
    let stem = base.strip_suffix(".framenapper").or_else(|| base.strip_suffix(".framenab")).unwrap_or(base);
    let mut root = dest_dir.join(format!("{stem}.framenapper"));
    let mut n = 2;
    while root.exists() {
        root = dest_dir.join(format!("{stem} ({n}).framenapper"));
        n += 1;
    }
    let store = FrameStore::create(&root)?;
    let result = (|| {
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(zip_err)?;
            if entry.is_dir() || !allowed(entry.name()) {
                continue;
            }
            let path = root.join(entry.name());
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).at(parent)?;
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(zip_err)?;
            fs::write(&path, bytes).at(&path)?;
        }
        let project = store.load_project()?;
        if let Some(frame) = project.frames.iter().find(|f| !store.frame_path(f).is_file()) {
            return Err(CoreError::Archive(format!("frame {} is missing", frame.file)));
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&root);
        return Err(e);
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn roundtrip_keeps_project_input_and_used_frames() {
        let dir = tempfile::tempdir().unwrap();
        let store = FrameStore::create(dir.path().join("a.framenapper")).unwrap();
        let mut p = Project::new(2, 1);
        for v in [1, 2, 3] {
            store.push_frame(&mut p, &RgbaImage::from_pixel(2, 1, image::Rgba([v, 0, 0, 255])), 50).unwrap();
        }
        crate::ops::delete_frames(&mut p, &[1]).unwrap();
        crate::ops::duplicate_frames(&mut p, &[0]).unwrap();
        let input =
            crate::InputEvents { keys: vec![crate::input::KeyEvent { t: 5, label: "A".into() }], ..Default::default() };
        store.save_input(&input).unwrap();

        let logo = dir.path().join("logo.png");
        RgbaImage::from_pixel(3, 2, image::Rgba([9, 8, 7, 255])).save(&logo).unwrap();
        let (file, w, h) = store.import_asset(&logo, "watermark").unwrap();
        assert_eq!((file.as_str(), w, h), ("assets/watermark-1.png", 3, 2));
        p.overlays.watermark = Some(crate::overlays::Watermark {
            file,
            image_width: w,
            image_height: h,
            anchor: crate::overlays::Anchor::Center,
            size: 0.5,
            opacity: 1.0,
        });

        let zip_path = dir.path().join("Demo.framenapper.zip");
        export_zip(&store, &p, &zip_path).unwrap();
        let names: Vec<String> =
            ZipArchive::new(fs::File::open(&zip_path).unwrap()).unwrap().file_names().map(String::from).collect();
        assert_eq!(names.len(), 5, "project, input, watermark and two distinct frames: {names:?}");

        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let root = import_zip(&zip_path, &out).unwrap();
        assert_eq!(root, out.join("Demo.framenapper"));
        let imported = FrameStore::create(&root).unwrap();
        assert_eq!(imported.load_project().unwrap(), p);
        assert_eq!(imported.load_input().unwrap(), input);
        assert_eq!(imported.load_frame(&p.frames[2]).unwrap().get_pixel(0, 0).0[0], 3);
        assert_eq!(imported.load_asset("assets/watermark-1.png").unwrap().get_pixel(0, 0).0, [9, 8, 7, 255]);
        // A second import gets its own folder.
        assert_eq!(import_zip(&zip_path, &out).unwrap(), out.join("Demo (2).framenapper"));
        // Archives from before the rename.
        let old = dir.path().join("Old.framenab.zip");
        fs::copy(&zip_path, &old).unwrap();
        assert_eq!(import_zip(&old, &out).unwrap(), out.join("Old.framenapper"));
    }

    #[test]
    fn rejects_unsafe_names_and_broken_archives() {
        assert!(allowed("frames/f000001.qoi"));
        assert!(!allowed("frames/../../etc/passwd"));
        assert!(!allowed("../project.json"));
        assert!(!allowed("frames/sub/x.qoi"));
        assert!(allowed("assets/watermark-1.png"));
        assert!(!allowed("assets/../x.png"));

        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.zip");
        let mut zip = ZipWriter::new(fs::File::create(&bad).unwrap());
        zip.start_file("project.json", SimpleFileOptions::default()).unwrap();
        let mut p = Project::new(1, 1);
        p.frames.push(crate::Frame {
            id: "f000001".into(),
            file: "frames/f000001.qoi".into(),
            delay_ms: 10,
            time_ms: None,
        });
        zip.write_all(&serde_json::to_vec(&p).unwrap()).unwrap();
        zip.finish().unwrap();
        assert!(import_zip(&bad, dir.path()).is_err());
        assert!(!dir.path().join("bad.framenapper").exists(), "a failed import leaves nothing behind");
    }
}
