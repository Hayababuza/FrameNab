//! Operations on the image of every frame: crop, resize, rotate, flip.
//!
//! Frames are immutable files, so an operation writes new frame files (with new IDs) and the
//! old ones stay on disk until the editor closes; undo just goes back to the previous project.
//! Annotations are transformed along with the image, and the recorded input (mouse, clicks)
//! is mapped through [`Project::input_transform`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use image::RgbaImage;
use image::imageops::{self, FilterType};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{CoreError, FrameStore, Point, Project, Rect, Result, Shape};

/// Largest width or height an operation may produce.
pub const MAX_SIZE: u32 = 8192;

/// An image operation for all frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum ImageOp {
    Crop {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    /// Scales to exactly `width` × `height` (Lanczos).
    Resize {
        width: u32,
        height: u32,
    },
    /// Rotates by 90°.
    Rotate {
        clockwise: bool,
    },
    Flip {
        horizontal: bool,
    },
}

/// Affine map of image coordinates: `x' = a·x + b·y + c`, `y' = d·x + e·y + f`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Transform {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Self = Self { a: 1.0, b: 0.0, c: 0.0, d: 0.0, e: 1.0, f: 0.0 };

    #[must_use]
    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    #[must_use]
    pub fn apply(&self, p: Point) -> Point {
        Point { x: self.a * p.x + self.b * p.y + self.c, y: self.d * p.x + self.e * p.y + self.f }
    }

    /// First `self`, then `next`.
    #[must_use]
    pub fn then(&self, next: &Self) -> Self {
        Self {
            a: next.a * self.a + next.b * self.d,
            b: next.a * self.b + next.b * self.e,
            c: next.a * self.c + next.b * self.f + next.c,
            d: next.d * self.a + next.e * self.d,
            e: next.d * self.b + next.e * self.e,
            f: next.d * self.c + next.e * self.f + next.f,
        }
    }

    /// How much lengths grow (for line widths, font sizes …).
    #[must_use]
    pub fn scale(&self) -> f32 {
        (self.a * self.e - self.b * self.d).abs().sqrt()
    }

    fn rect(&self, r: Rect) -> Rect {
        let p = self.apply(Point { x: r.x, y: r.y });
        let q = self.apply(Point { x: r.x + r.w, y: r.y + r.h });
        Rect { x: p.x, y: p.y, w: q.x - p.x, h: q.y - p.y }.normalized()
    }
}

impl ImageOp {
    /// Size of the result for a `width` × `height` image.
    #[must_use]
    pub fn size(&self, width: u32, height: u32) -> (u32, u32) {
        match *self {
            Self::Crop { width, height, .. } | Self::Resize { width, height } => (width, height),
            Self::Rotate { .. } => (height, width),
            Self::Flip { .. } => (width, height),
        }
    }

    fn validate(&self, width: u32, height: u32) -> Result<()> {
        let (w, h) = self.size(width, height);
        let crop_inside = match *self {
            Self::Crop { x, y, width: cw, height: ch } => {
                x.checked_add(cw).is_some_and(|r| r <= width) && y.checked_add(ch).is_some_and(|b| b <= height)
            }
            _ => true,
        };
        if w == 0 || h == 0 || w > MAX_SIZE || h > MAX_SIZE || !crop_inside {
            return Err(CoreError::InvalidImageOp);
        }
        Ok(())
    }

    /// How coordinates of a `width` × `height` image move.
    #[must_use]
    pub fn transform(&self, width: u32, height: u32) -> Transform {
        let (w, h) = (width as f32, height as f32);
        let t = |a, b, c, d, e, f| Transform { a, b, c, d, e, f };
        match *self {
            Self::Crop { x, y, .. } => t(1.0, 0.0, -(x as f32), 0.0, 1.0, -(y as f32)),
            Self::Resize { width: nw, height: nh } => t(nw as f32 / w, 0.0, 0.0, 0.0, nh as f32 / h, 0.0),
            Self::Rotate { clockwise: true } => t(0.0, -1.0, h, 1.0, 0.0, 0.0),
            Self::Rotate { clockwise: false } => t(0.0, 1.0, 0.0, -1.0, 0.0, w),
            Self::Flip { horizontal: true } => t(-1.0, 0.0, w, 0.0, 1.0, 0.0),
            Self::Flip { horizontal: false } => t(1.0, 0.0, 0.0, 0.0, -1.0, h),
        }
    }

    /// Applies the operation to one image.
    #[must_use]
    pub fn apply(&self, image: &RgbaImage) -> RgbaImage {
        match *self {
            Self::Crop { x, y, width, height } => imageops::crop_imm(image, x, y, width, height).to_image(),
            Self::Resize { width, height } => imageops::resize(image, width, height, FilterType::Lanczos3),
            Self::Rotate { clockwise: true } => imageops::rotate90(image),
            Self::Rotate { clockwise: false } => imageops::rotate270(image),
            Self::Flip { horizontal: true } => imageops::flip_horizontal(image),
            Self::Flip { horizontal: false } => imageops::flip_vertical(image),
        }
    }
}

impl Shape {
    /// The shape mapped through `t`; sizes (font, blur radius …) scale with it.
    #[must_use]
    pub fn transformed(&self, t: &Transform) -> Self {
        let s = t.scale();
        let mut shape = self.clone();
        match &mut shape {
            Self::Arrow { from, to } | Self::Line { from, to } => {
                *from = t.apply(*from);
                *to = t.apply(*to);
            }
            Self::Rect { rect } | Self::Ellipse { rect } => *rect = t.rect(*rect),
            Self::Blur { rect, radius } => {
                *rect = t.rect(*rect);
                *radius *= s;
            }
            Self::Pixelate { rect, block_size } => {
                *rect = t.rect(*rect);
                *block_size = ((*block_size as f32 * s).round() as u32).max(1);
            }
            Self::Pen { points } | Self::Highlighter { points } => {
                points.iter_mut().for_each(|p| *p = t.apply(*p));
            }
            Self::Text { at, font_size, .. } => {
                *at = t.apply(*at);
                *font_size *= s;
            }
            Self::Counter { at, .. } | Self::Click { at } => *at = t.apply(*at),
        }
        shape
    }
}

/// Updates size, annotations and the input mapping of `project` for `op` (not the frames).
pub fn transform_project(project: &mut Project, op: &ImageOp) -> Result<()> {
    op.validate(project.width, project.height)?;
    let t = op.transform(project.width, project.height);
    let s = t.scale();
    for a in &mut project.annotations {
        a.shape = a.shape.transformed(&t);
        a.style.width *= s;
    }
    project.input_transform = project.input_transform.then(&t);
    (project.width, project.height) = op.size(project.width, project.height);
    Ok(())
}

/// Applies `op` to every frame: writes new frame files (in parallel) and updates the project.
/// `done` counts processed images (frames sharing a file count once).
pub fn apply_image_op(store: &FrameStore, project: &mut Project, op: &ImageOp, done: &AtomicUsize) -> Result<()> {
    op.validate(project.width, project.height)?;
    // One new file per distinct source file (duplicated frames share theirs).
    let mut jobs: Vec<(usize, String)> = Vec::new();
    let mut new_file: HashMap<String, String> = HashMap::new();
    for (i, frame) in project.frames.iter().enumerate() {
        if !new_file.contains_key(&frame.file) {
            new_file.insert(frame.file.clone(), String::new());
            jobs.push((i, frame.file.clone()));
        }
    }
    for (_, old) in &jobs {
        let id = project.allocate_frame_id();
        new_file.insert(old.clone(), store.frame_file_name(&id));
    }

    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get).min(16);
    let next = AtomicUsize::new(0);
    let frames = &project.frames;
    std::thread::scope(|scope| -> Result<()> {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| -> Result<()> {
                    loop {
                        let Some((index, old)) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) else {
                            return Ok(());
                        };
                        let image = store.load_frame(&frames[*index])?;
                        store.write_image(&new_file[old], &op.apply(&image))?;
                        done.fetch_add(1, Ordering::Relaxed);
                    }
                })
            })
            .collect();
        workers.into_iter().try_for_each(|w| w.join().unwrap_or(Err(CoreError::InvalidImageOp)))
    })?;

    transform_project(project, op)?;
    let frames: Vec<_> = project.frames.clone();
    project.frames = frames
        .into_iter()
        .map(|mut frame| {
            frame.file = new_file[&frame.file].clone();
            frame.id = project.allocate_frame_id();
            frame
        })
        .collect();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Annotation, InputEvents, Style, input::CursorSample};

    fn close(p: Point, x: f32, y: f32) -> bool {
        (p.x - x).abs() < 1e-4 && (p.y - y).abs() < 1e-4
    }

    #[test]
    fn transforms_match_the_image_operations() {
        // A 4×2 image with one marked pixel at (3, 0).
        let mut image = RgbaImage::new(4, 2);
        image.put_pixel(3, 0, image::Rgba([255, 0, 0, 255]));
        let marked = |im: &RgbaImage| {
            im.enumerate_pixels().find(|(_, _, p)| p.0[0] == 255).map(|(x, y, _)| (x as f32 + 0.5, y as f32 + 0.5))
        };
        for op in [
            ImageOp::Rotate { clockwise: true },
            ImageOp::Rotate { clockwise: false },
            ImageOp::Flip { horizontal: true },
            ImageOp::Flip { horizontal: false },
            ImageOp::Crop { x: 1, y: 0, width: 3, height: 2 },
        ] {
            let out = op.apply(&image);
            assert_eq!(out.dimensions(), op.size(4, 2), "{op:?}");
            let (x, y) = marked(&out).expect("pixel kept");
            // Pixel centers map onto pixel centers.
            assert!(close(op.transform(4, 2).apply(Point { x: 3.5, y: 0.5 }), x, y), "{op:?}");
        }
    }

    #[test]
    fn transforms_compose() {
        let crop = ImageOp::Crop { x: 10, y: 20, width: 100, height: 50 }.transform(200, 100);
        let half = ImageOp::Resize { width: 50, height: 25 }.transform(100, 50);
        let both = crop.then(&half);
        assert!(close(both.apply(Point { x: 30.0, y: 40.0 }), 10.0, 10.0));
        assert!((both.scale() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn project_annotations_and_input_follow() {
        let mut p = Project::new(200, 100);
        p.annotations.push(Annotation {
            id: "a".into(),
            frame_start: 0,
            frame_end: 0,
            shape: Shape::Rect { rect: Rect { x: 20.0, y: 30.0, w: 40.0, h: 10.0 } },
            style: Style::default(),
        });
        transform_project(&mut p, &ImageOp::Resize { width: 100, height: 50 }).unwrap();
        transform_project(&mut p, &ImageOp::Rotate { clockwise: true }).unwrap();
        assert_eq!((p.width, p.height), (50, 100));
        // Rect (10,15)-(30,20) after the resize, rotated clockwise in a 100×50 image.
        assert_eq!(p.annotations[0].shape, Shape::Rect { rect: Rect { x: 30.0, y: 10.0, w: 5.0, h: 20.0 } });
        assert!((p.annotations[0].style.width - 2.0).abs() < 1e-6);

        let input = InputEvents { cursor: vec![CursorSample { t: 0, x: 20.0, y: 30.0 }], ..Default::default() };
        let mapped = input.transformed(&p.input_transform);
        assert!(close(Point { x: mapped.cursor[0].x, y: mapped.cursor[0].y }, 35.0, 10.0));
    }

    #[test]
    fn invalid_operations_are_rejected() {
        let mut p = Project::new(100, 100);
        assert!(transform_project(&mut p, &ImageOp::Crop { x: 50, y: 0, width: 60, height: 10 }).is_err());
        assert!(transform_project(&mut p, &ImageOp::Resize { width: 0, height: 10 }).is_err());
        assert_eq!((p.width, p.height), (100, 100));
    }

    #[test]
    fn writes_new_frames_once_per_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = FrameStore::create(dir.path()).unwrap();
        let mut p = Project::new(4, 2);
        store.push_frame(&mut p, &RgbaImage::new(4, 2), 100).unwrap();
        crate::ops::duplicate_frames(&mut p, &[0]).unwrap();
        let before = p.clone();
        let done = AtomicUsize::new(0);
        apply_image_op(&store, &mut p, &ImageOp::Rotate { clockwise: true }, &done).unwrap();
        assert_eq!(done.load(Ordering::Relaxed), 1);
        assert_eq!((p.width, p.height), (2, 4));
        assert_eq!(p.frames[0].file, p.frames[1].file);
        assert_ne!(p.frames[0].file, before.frames[0].file);
        assert_ne!(p.frames[0].id, p.frames[1].id);
        assert_eq!(store.load_frame(&p.frames[1]).unwrap().dimensions(), (2, 4));
        // The old files stay for undo.
        assert!(store.load_frame(&before.frames[0]).is_ok());
    }
}
