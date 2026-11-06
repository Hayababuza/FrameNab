//! Editor frame operations (delete, duplicate, reverse, yo-yo, timing …).
//!
//! Every operation that changes the frame order goes through [`apply_order`], which keeps the
//! frame ranges of annotations consistent.

use crate::{CoreError, Project, Result};

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
    let mut keep = Vec::new();
    for i in identity(project) {
        if (i + 1) % n == 0 {
            if let Some(&prev) = keep.last() {
                let removed = project.frames[i].delay_ms;
                let frame: &mut crate::Frame = &mut project.frames[prev];
                frame.delay_ms = frame.delay_ms.saturating_add(removed);
            }
        } else {
            keep.push(i);
        }
    }
    apply_order(project, &keep)
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
            p.frames.push(Frame { file: format!("frames/{id}.qoi"), id, delay_ms: 100 });
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
    fn out_of_range_is_error() {
        let mut p = project(2);
        assert!(apply_order(&mut p, &[0, 5]).is_err());
    }
}
