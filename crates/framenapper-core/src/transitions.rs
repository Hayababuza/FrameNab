//! Transitions: frames inserted between two frames that fade or slide from one to the other.

use image::{Rgba as Pixel, RgbaImage};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{CoreError, Frame, FrameStore, Project, Result, Rgba};

/// Most frames a single transition may insert.
pub const MAX_FRAMES: u32 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum Transition {
    /// Cross-fades into the next frame.
    Fade,
    /// Fades into a solid color (e.g. to black at the end).
    FadeToColor { color: Rgba },
    /// The next frame pushes the current one out, moving in `direction`.
    Slide { direction: Direction },
}

/// Inserts `frames` transition frames after frame `after`, lasting `duration_ms` in total.
/// The frame after the last one is the first (the animation loops).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TransitionOp {
    pub after: u32,
    pub transition: Transition,
    pub frames: u32,
    pub duration_ms: u32,
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: u8, b: u8, t: f32) -> u8 {
    (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round().clamp(0.0, 255.0) as u8
}

/// The transition from `from` to `to` at `t` (0 = `from`, 1 = `to`). Both images have the
/// same size.
#[must_use]
pub fn render(transition: Transition, from: &RgbaImage, to: &RgbaImage, t: f32) -> RgbaImage {
    let t = t.clamp(0.0, 1.0);
    let (w, h) = from.dimensions();
    match transition {
        Transition::Fade => RgbaImage::from_fn(w, h, |x, y| {
            let (a, b) = (from.get_pixel(x, y).0, to.get_pixel(x, y).0);
            Pixel(std::array::from_fn(|i| mix(a[i], b[i], t)))
        }),
        Transition::FadeToColor { color } => {
            let c = [color.r, color.g, color.b, color.a];
            RgbaImage::from_fn(w, h, |x, y| {
                let a = from.get_pixel(x, y).0;
                Pixel(std::array::from_fn(|i| mix(a[i], c[i], t)))
            })
        }
        Transition::Slide { direction } => {
            let s = smoothstep(t);
            // How far the content has moved, in pixels.
            let dx = (w as f32 * s).round() as i64;
            let dy = (h as f32 * s).round() as i64;
            let (w, h) = (i64::from(w), i64::from(h));
            RgbaImage::from_fn(from.width(), from.height(), |x, y| {
                let (x, y) = (i64::from(x), i64::from(y));
                // Position in the strip [from | to] laid out in the direction of movement.
                let (from_xy, to_xy) = match direction {
                    Direction::Left => ((x + dx, y), (x + dx - w, y)),
                    Direction::Right => ((x - dx, y), (x - dx + w, y)),
                    Direction::Up => ((x, y + dy), (x, y + dy - h)),
                    Direction::Down => ((x, y - dy), (x, y - dy + h)),
                };
                let inside = |(px, py): (i64, i64)| (0..w).contains(&px) && (0..h).contains(&py);
                if inside(from_xy) {
                    *from.get_pixel(from_xy.0 as u32, from_xy.1 as u32)
                } else if inside(to_xy) {
                    *to.get_pixel(to_xy.0 as u32, to_xy.1 as u32)
                } else {
                    Pixel([0, 0, 0, 255])
                }
            })
        }
    }
}

/// Renders and inserts the transition frames, writing their images to `store`.
pub fn insert_transition(store: &FrameStore, project: &mut Project, op: &TransitionOp) -> Result<()> {
    let len = project.frames.len();
    let after = op.after as usize;
    if after >= len {
        return Err(CoreError::FrameOutOfRange { index: after, len });
    }
    if op.frames == 0 || op.frames > MAX_FRAMES {
        return Err(CoreError::InvalidImageOp);
    }
    let from_frame = project.frames[after].clone();
    let from = store.load_frame(&from_frame)?;
    let to = match op.transition {
        Transition::FadeToColor { .. } => None,
        _ => Some(store.load_frame(&project.frames[(after + 1) % len])?),
    };
    let n = op.frames;
    let delay = (op.duration_ms / n).max(1);

    let mut inserted = Vec::with_capacity(n as usize);
    for k in 1..=n {
        let t = k as f32 / (n + 1) as f32;
        let image = render(op.transition, &from, to.as_ref().unwrap_or(&from), t);
        let id = project.allocate_frame_id();
        let file = store.frame_file_name(&id);
        store.write_image(&file, &image)?;
        // Same moment as the frame before: the mouse effects stay where they were.
        inserted.push(Frame { id, file, delay_ms: delay, time_ms: from_frame.time_ms });
    }
    project.frames.splice(after + 1..after + 1, inserted);

    // Annotations after the insert point move back; ones spanning it grow.
    for a in &mut project.annotations {
        if a.frame_start > op.after {
            a.frame_start += n;
        }
        if a.frame_end > op.after {
            a.frame_end += n;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Annotation, Point, Shape, Style};

    fn solid(v: u8) -> RgbaImage {
        RgbaImage::from_pixel(4, 2, Pixel([v, v, v, 255]))
    }

    #[test]
    fn fade_mixes_and_reaches_the_target() {
        let (a, b) = (solid(0), solid(200));
        assert_eq!(render(Transition::Fade, &a, &b, 0.5).get_pixel(0, 0).0, [100, 100, 100, 255]);
        assert_eq!(render(Transition::Fade, &a, &b, 1.0), b);
        let white = Transition::FadeToColor { color: Rgba { r: 255, g: 255, b: 255, a: 255 } };
        assert_eq!(render(white, &a, &a, 1.0), solid(255));
    }

    #[test]
    fn slide_pushes_the_next_frame_in() {
        let (a, b) = (solid(0), solid(200));
        let half = |d| render(Transition::Slide { direction: d }, &a, &b, 0.5);
        // Moving left: the next frame enters from the right.
        let left = half(Direction::Left);
        assert_eq!((left.get_pixel(0, 0).0[0], left.get_pixel(3, 0).0[0]), (0, 200));
        let right = half(Direction::Right);
        assert_eq!((right.get_pixel(0, 0).0[0], right.get_pixel(3, 0).0[0]), (200, 0));
        let up = half(Direction::Up);
        assert_eq!((up.get_pixel(0, 0).0[0], up.get_pixel(0, 1).0[0]), (0, 200));
        assert_eq!(render(Transition::Slide { direction: Direction::Down }, &a, &b, 1.0), b);
    }

    #[test]
    fn inserts_frames_and_moves_annotations() {
        let dir = tempfile::tempdir().unwrap();
        let store = FrameStore::create(dir.path()).unwrap();
        let mut p = Project::new(4, 2);
        for v in [0, 200, 50] {
            store.push_frame(&mut p, &solid(v), 100).unwrap();
        }
        p.frames[0].time_ms = Some(40);
        for (start, end) in [(0, 0), (0, 1), (2, 2)] {
            p.annotations.push(Annotation {
                id: format!("a{start}{end}"),
                frame_start: start,
                frame_end: end,
                shape: Shape::Click { at: Point::default() },
                style: Style::default(),
            });
        }
        let op = TransitionOp { after: 0, transition: Transition::Fade, frames: 3, duration_ms: 300 };
        insert_transition(&store, &mut p, &op).unwrap();
        assert_eq!(p.frames.len(), 6);
        assert_eq!(store.load_frame(&p.frames[2]).unwrap().get_pixel(0, 0).0[0], 100);
        assert!(p.frames[1..4].iter().all(|f| f.delay_ms == 100 && f.time_ms == Some(40)));
        let ranges: Vec<_> = p.annotations.iter().map(|a| (a.frame_start, a.frame_end)).collect();
        assert_eq!(ranges, [(0, 0), (0, 4), (5, 5)]);

        // After the last frame, the transition leads back to the first one.
        let last = TransitionOp { after: 5, transition: Transition::Fade, frames: 1, duration_ms: 50 };
        insert_transition(&store, &mut p, &last).unwrap();
        assert_eq!(store.load_frame(&p.frames[6]).unwrap().get_pixel(0, 0).0[0], 25);
        let bad = TransitionOp { after: 9, ..last };
        assert!(insert_transition(&store, &mut p, &bad).is_err());
    }
}
