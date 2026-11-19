//! Editor frame operations (delete, duplicate, reverse, yo-yo, timing …).
//!
//! Every operation that changes the frame order goes through [`apply_order`], which keeps the
//! frame ranges of annotations consistent.

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{CoreError, Frame, Project, Result};

/// A frame operation requested by the editor. `frames` are frame indices (the selection).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum FrameOp {
    Delete {
        frames: Vec<u32>,
    },
    Duplicate {
        frames: Vec<u32>,
    },
    /// Reverses the selected range (first to last selected frame), or everything if fewer than
    /// two frames are selected.
    Reverse {
        frames: Vec<u32>,
    },
    Yoyo,
    /// Moves the frames so they start at position `to` (an index of the current order,
    /// `0..=len`; the frames keep their relative order).
    Move {
        frames: Vec<u32>,
        to: u32,
    },
    RemoveEveryNth {
        n: u32,
    },
    SetDelay {
        frames: Vec<u32>,
        ms: u32,
    },
    ScaleDelay {
        frames: Vec<u32>,
        percent: u32,
    },
    /// Removes frames that look like the frame before them (`threshold` = percent of identical
    /// pixels, 100 = only exact copies); their time goes to the frame they repeat. Needs the
    /// images, see [`similar_frames`].
    RemoveSimilar {
        threshold: f32,
    },
}

fn indices(frames: &[u32]) -> Vec<usize> {
    frames.iter().map(|&i| i as usize).collect()
}

/// Applies `op`. [`FrameOp::RemoveSimilar`] needs the frame images: `load` gives them.
pub fn apply(project: &mut Project, op: &FrameOp, load: impl FnMut(&Frame) -> Result<RgbaImage>) -> Result<()> {
    match op {
        FrameOp::Delete { frames } => delete_frames(project, &indices(frames)),
        FrameOp::Duplicate { frames } => duplicate_frames(project, &indices(frames)),
        FrameOp::Reverse { frames } => {
            let list = indices(frames);
            match (list.iter().min(), list.iter().max()) {
                (Some(&a), Some(&b)) if b > a => reverse(project, a, b),
                _ if project.frames.len() > 1 => reverse(project, 0, project.frames.len() - 1),
                _ => Ok(()),
            }
        }
        FrameOp::Yoyo => yoyo(project),
        FrameOp::Move { frames, to } => move_frames(project, &indices(frames), *to as usize),
        FrameOp::RemoveEveryNth { n } => remove_every_nth(project, *n as usize),
        FrameOp::SetDelay { frames, ms } => {
            set_delay(project, &indices(frames), *ms);
            Ok(())
        }
        FrameOp::ScaleDelay { frames, percent } => {
            scale_delay(project, &indices(frames), *percent);
            Ok(())
        }
        FrameOp::RemoveSimilar { threshold } => {
            let similar = similar_frames(project, *threshold, load)?;
            merge_into_previous(project, &similar)
        }
    }
}

/// Reorders frames. `order[new] = old` (indices may be missing or appear more than once).
///
/// An annotation that covered the old frames `start..=end` afterwards covers the smallest range
/// containing every new frame whose source lies in `start..=end`. If no such frame remains, the
/// annotation is removed.
pub fn apply_order(project: &mut Project, order: &[usize]) -> Result<()> {
    let len = project.frames.len();
    if let Some(&bad) = order.iter().find(|&&i| i >= len) {
        return Err(CoreError::FrameOutOfRange { index: bad, len });
    }

    let mut seen = vec![false; len];
    let mut frames = Vec::with_capacity(order.len());
    for &old in order {
        let mut frame = project.frames[old].clone();
        if seen[old] {
            // Duplicate: new ID, same image file (frames are immutable).
            frame.id = project.allocate_frame_id();
        }
        seen[old] = true;
        frames.push(frame);
    }

    project.annotations.retain_mut(|a| {
        let range = a.frame_start as usize..=a.frame_end as usize;
        let mut hits = order.iter().enumerate().filter(|(_, old)| range.contains(old)).map(|(new, _)| new);
        let Some(first) = hits.next() else { return false };
        let last = hits.next_back().unwrap_or(first);
        a.frame_start = u32::try_from(first).unwrap_or(u32::MAX);
        a.frame_end = u32::try_from(last).unwrap_or(u32::MAX);
        true
    });

    project.frames = frames;
    Ok(())
}

fn identity(project: &Project) -> Vec<usize> {
    (0..project.frames.len()).collect()
}

/// Deletes the given frames.
pub fn delete_frames(project: &mut Project, indices: &[usize]) -> Result<()> {
    let order: Vec<usize> = identity(project).into_iter().filter(|i| !indices.contains(i)).collect();
    apply_order(project, &order)
}

/// Duplicates the given frames; each copy is placed right after its original.
pub fn duplicate_frames(project: &mut Project, indices: &[usize]) -> Result<()> {
    let mut order = Vec::new();
    for i in identity(project) {
        order.push(i);
        if indices.contains(&i) {
            order.push(i);
        }
    }
    apply_order(project, &order)
}

/// Reverses the order within `start..=end`.
pub fn reverse(project: &mut Project, start: usize, end: usize) -> Result<()> {
    let mut order = identity(project);
    let len = order.len();
    if end >= len || start > end {
        return Err(CoreError::FrameOutOfRange { index: end, len });
    }
    order[start..=end].reverse();
    apply_order(project, &order)
}

/// Yo-yo: appends the frames in reverse (without showing the last frame twice).
pub fn yoyo(project: &mut Project) -> Result<()> {
    let mut order = identity(project);
    order.extend(identity(project).into_iter().rev().skip(1));
    apply_order(project, &order)
}

/// Removes every `n`-th frame (1-based: n=2 removes frames 2, 4, 6 …).
/// The duration of a removed frame is added to the preceding frame.
pub fn remove_every_nth(project: &mut Project, n: usize) -> Result<()> {
    if n < 2 {
        return Ok(());
    }
    let remove: Vec<usize> = identity(project).into_iter().filter(|i| (i + 1) % n == 0).collect();
    merge_into_previous(project, &remove)
}

/// Removes the given frames and adds their duration to the closest kept frame before them (or,
/// for leading frames, after them), so the total duration stays the same. At least one frame
/// is kept.
pub fn merge_into_previous(project: &mut Project, remove: &[usize]) -> Result<()> {
    let len = project.frames.len();
    let mut keep: Vec<usize> = (0..len).filter(|i| !remove.contains(i)).collect();
    if keep.is_empty() {
        if len == 0 {
            return Ok(());
        }
        keep.push(0);
    }
    let mut pending = 0u32;
    for i in 0..len {
        let delay = project.frames[i].delay_ms;
        if keep.binary_search(&i).is_ok() {
            continue;
        }
        match keep.iter().rev().find(|&&k| k < i) {
            Some(&prev) => {
                let frame = &mut project.frames[prev];
                frame.delay_ms = frame.delay_ms.saturating_add(delay);
            }
            None => pending = pending.saturating_add(delay),
        }
    }
    let first = &mut project.frames[keep[0]];
    first.delay_ms = first.delay_ms.saturating_add(pending);
    apply_order(project, &keep)
}

/// Moves `indices` so they start at position `to` of the current order (`0..=len`).
pub fn move_frames(project: &mut Project, indices: &[usize], to: usize) -> Result<()> {
    let len = project.frames.len();
    if let Some(&bad) = indices.iter().find(|&&i| i >= len) {
        return Err(CoreError::FrameOutOfRange { index: bad, len });
    }
    let mut moved: Vec<usize> = indices.to_vec();
    moved.sort_unstable();
    moved.dedup();
    let rest = (0..len).filter(|i| !moved.contains(i));
    let (before, after): (Vec<usize>, Vec<usize>) = rest.partition(|&i| i < to.min(len));
    let order: Vec<usize> = before.into_iter().chain(moved).chain(after).collect();
    apply_order(project, &order)
}

/// Percentage of identical pixels (0–100); images of different size are 0 % similar.
#[must_use]
pub fn similarity(a: &RgbaImage, b: &RgbaImage) -> f32 {
    if a.dimensions() != b.dimensions() || a.is_empty() {
        return 0.0;
    }
    let same = a.as_raw().as_chunks::<4>().0.iter().zip(b.as_raw().as_chunks::<4>().0).filter(|(p, q)| p == q).count();
    same as f32 * 100.0 / (a.width() * a.height()) as f32
}

/// Frames (indices) that are at least `threshold` % identical to the last frame kept before
/// them. Frames that share an image file are identical without loading it.
pub fn similar_frames(
    project: &Project,
    threshold: f32,
    mut load: impl FnMut(&Frame) -> Result<RgbaImage>,
) -> Result<Vec<usize>> {
    let mut similar = Vec::new();
    let Some(first) = project.frames.first() else { return Ok(similar) };
    let mut kept_file = &first.file;
    let mut kept: Option<RgbaImage> = None;
    for (i, frame) in project.frames.iter().enumerate().skip(1) {
        if frame.file == *kept_file {
            similar.push(i);
            continue;
        }
        let image = load(frame)?;
        let reference = match kept.take() {
            Some(image) => image,
            None => load(project.frames.iter().find(|f| f.file == *kept_file).unwrap_or(first))?,
        };
        if similarity(&reference, &image) >= threshold {
            similar.push(i);
            kept = Some(reference);
        } else {
            kept_file = &frame.file;
            kept = Some(image);
        }
    }
    Ok(similar)
}

/// Sets the display duration of the given frames.
pub fn set_delay(project: &mut Project, indices: &[usize], delay_ms: u32) {
    for &i in indices {
        if let Some(f) = project.frames.get_mut(i) {
            f.delay_ms = delay_ms.max(1);
        }
    }
}

/// Scales the display duration by `percent` (200 = twice as long, 50 = half as long).
pub fn scale_delay(project: &mut Project, indices: &[usize], percent: u32) {
    for &i in indices {
        if let Some(f) = project.frames.get_mut(i) {
            let scaled = u64::from(f.delay_ms) * u64::from(percent) / 100;
            f.delay_ms = u32::try_from(scaled).unwrap_or(u32::MAX).max(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Annotation, Frame, Point, Shape, Style};

    fn project(n: usize) -> Project {
        let mut p = Project::new(4, 4);
        for _ in 0..n {
            let id = p.allocate_frame_id();
            p.frames.push(Frame { file: format!("frames/{id}.qoi"), id, delay_ms: 100, time_ms: None });
        }
        p
    }

    fn annotate(p: &mut Project, start: u32, end: u32) {
        p.annotations.push(Annotation {
            id: format!("a{}", p.annotations.len()),
            frame_start: start,
            frame_end: end,
            shape: Shape::Counter { at: Point::default(), value: 1 },
            style: Style::default(),
        });
    }

    fn ids(p: &Project) -> Vec<&str> {
        p.frames.iter().map(|f| f.id.as_str()).collect()
    }

    #[test]
    fn delete_shrinks_and_drops_annotations() {
        let mut p = project(5);
        annotate(&mut p, 1, 3);
        annotate(&mut p, 4, 4);
        delete_frames(&mut p, &[2, 4]).unwrap();
        assert_eq!(ids(&p), ["f000001", "f000002", "f000004"]);
        assert_eq!(p.annotations.len(), 1);
        assert_eq!((p.annotations[0].frame_start, p.annotations[0].frame_end), (1, 2));
    }

    #[test]
    fn duplicate_creates_unique_ids_and_shares_file() {
        let mut p = project(2);
        duplicate_frames(&mut p, &[0]).unwrap();
        assert_eq!(ids(&p), ["f000001", "f000003", "f000002"]);
        assert_eq!(p.frames[0].file, p.frames[1].file);
    }

    #[test]
    fn yoyo_and_reverse() {
        let mut p = project(3);
        yoyo(&mut p).unwrap();
        assert_eq!(p.frames.len(), 5);
        let mut p = project(4);
        annotate(&mut p, 0, 0);
        reverse(&mut p, 0, 3).unwrap();
        assert_eq!(ids(&p), ["f000004", "f000003", "f000002", "f000001"]);
        assert_eq!(p.annotations[0].frame_start, 3);
    }

    #[test]
    fn remove_every_nth_keeps_duration() {
        let mut p = project(6);
        let before = p.duration_ms();
        remove_every_nth(&mut p, 2).unwrap();
        assert_eq!(p.frames.len(), 3);
        assert_eq!(p.duration_ms(), before);
    }

    #[test]
    fn delays() {
        let mut p = project(2);
        scale_delay(&mut p, &[0], 50);
        set_delay(&mut p, &[1], 0);
        assert_eq!((p.frames[0].delay_ms, p.frames[1].delay_ms), (50, 1));
    }

    #[test]
    fn merge_into_previous_keeps_the_duration() {
        let mut p = project(4);
        merge_into_previous(&mut p, &[0, 2]).unwrap();
        assert_eq!(ids(&p), ["f000002", "f000004"]);
        assert_eq!((p.frames[0].delay_ms, p.frames[1].delay_ms), (300, 100));
        let mut p = project(2);
        merge_into_previous(&mut p, &[0, 1]).unwrap();
        assert_eq!((p.frames.len(), p.frames[0].delay_ms), (1, 200));
    }

    #[test]
    fn move_frames_keeps_their_order() {
        let mut p = project(5);
        move_frames(&mut p, &[3, 1], 0).unwrap();
        assert_eq!(ids(&p), ["f000002", "f000004", "f000001", "f000003", "f000005"]);
        let mut p = project(5);
        move_frames(&mut p, &[0, 1], 5).unwrap();
        assert_eq!(ids(&p), ["f000003", "f000004", "f000005", "f000001", "f000002"]);
        let mut p = project(5);
        move_frames(&mut p, &[0], 3).unwrap();
        assert_eq!(ids(&p), ["f000002", "f000003", "f000001", "f000004", "f000005"]);
    }

    #[test]
    fn removes_similar_frames_compared_with_the_last_kept_one() {
        // Frame images: a, a, almost a (1 of 4 pixels differs), b, b (shared file).
        let mut p = project(5);
        p.frames[4].file = p.frames[3].file.clone();
        let image = |v: u8, odd: bool| {
            let mut im = RgbaImage::from_pixel(2, 2, image::Rgba([v, v, v, 255]));
            if odd {
                im.put_pixel(0, 0, image::Rgba([9, 9, 9, 255]));
            }
            im
        };
        let mut loads = 0;
        let similar = similar_frames(&p, 75.0, |f| {
            loads += 1;
            Ok(match f.id.as_str() {
                "f000001" | "f000002" => image(1, false),
                "f000003" => image(1, true),
                _ => image(2, false),
            })
        })
        .unwrap();
        assert_eq!(similar, [1, 2, 4]);
        assert!(loads <= 4, "frames sharing a file are not loaded: {loads}");
        let exact = similar_frames(&p, 100.0, |f| Ok(image(1, f.id == "f000003"))).unwrap();
        assert_eq!(exact, [1, 4]);

        apply(&mut p, &FrameOp::RemoveSimilar { threshold: 75.0 }, |f| {
            Ok(if f.id < "f000004".into() { image(1, false) } else { image(2, false) })
        })
        .unwrap();
        assert_eq!(ids(&p), ["f000001", "f000004"]);
        assert_eq!(p.duration_ms(), 500);
    }

    #[test]
    fn reverse_uses_the_selected_range_or_everything() {
        let none = |_: &Frame| -> Result<RgbaImage> { unreachable!() };
        let mut p = project(4);
        apply(&mut p, &FrameOp::Reverse { frames: vec![1, 2] }, none).unwrap();
        assert_eq!(ids(&p), ["f000001", "f000003", "f000002", "f000004"]);
        let mut p = project(3);
        apply(&mut p, &FrameOp::Reverse { frames: vec![1] }, none).unwrap();
        assert_eq!(ids(&p), ["f000003", "f000002", "f000001"]);
    }

    #[test]
    fn out_of_range_is_error() {
        let mut p = project(2);
        assert!(apply_order(&mut p, &[0, 5]).is_err());
    }
}
