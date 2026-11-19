//! Overlays over the whole recording: a progress bar and a watermark image. Unlike annotations
//! they are settings of the project, not drawn shapes: the bar grows from frame to frame, the
//! watermark sits in a corner and keeps its place when the image is cropped or resized.
//!
//! `src/lib/annotations/overlays.ts` mirrors the geometry for the editor preview.

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Annotation, Project, Rect, Rgba, Shape, Style};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Overlays {
    pub progress: Option<ProgressBar>,
    pub watermark: Option<Watermark>,
}

impl Overlays {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.progress.is_none() && self.watermark.is_none()
    }
}

/// A bar along the top or bottom edge that fills up over the length of the animation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProgressBar {
    pub bottom: bool,
    /// Thickness in image pixels.
    pub height: f32,
    pub color: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

/// An image (e.g. a logo) placed on every frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Watermark {
    /// PNG inside the project folder, e.g. `assets/watermark-1.png`.
    pub file: String,
    pub image_width: u32,
    pub image_height: u32,
    pub anchor: Anchor,
    /// Width as a fraction of the frame width (0.05–1).
    pub size: f32,
    pub opacity: f32,
}

impl Project {
    /// The progress bar on frame `index` (full at the end of the last frame), if switched on.
    #[must_use]
    pub fn progress_annotation(&self, index: usize) -> Option<Annotation> {
        let bar = self.overlays.progress?;
        let total = self.duration_ms().max(1) as f32;
        let end: u64 = self.frames.iter().take(index + 1).map(|f| u64::from(f.delay_ms)).sum();
        let h = bar.height.clamp(1.0, self.height as f32);
        let y = if bar.bottom { self.height as f32 - h } else { 0.0 };
        Some(Annotation {
            id: "overlay-progress".into(),
            frame_start: 0,
            frame_end: u32::MAX,
            shape: Shape::Rect { rect: Rect { x: 0.0, y, w: self.width as f32 * (end as f32 / total).min(1.0), h } },
            style: Style { stroke: bar.color, width: 0.0, fill: Some(bar.color), opacity: 1.0 },
        })
    }
}

/// Where the watermark goes on a `width` × `height` frame (whole pixels).
#[must_use]
pub fn watermark_rect(width: u32, height: u32, wm: &Watermark) -> Rect {
    let (fw, fh) = (width as f32, height as f32);
    let aspect = wm.image_height.max(1) as f32 / wm.image_width.max(1) as f32;
    let mut w = (fw * wm.size.clamp(0.01, 1.0)).round().max(1.0);
    let mut h = (w * aspect).round().max(1.0);
    if h > fh {
        h = fh;
        w = (h / aspect).round().max(1.0);
    }
    let margin = (fw.min(fh) * 0.02).round();
    let (x, y) = match wm.anchor {
        Anchor::TopLeft => (margin, margin),
        Anchor::TopRight => (fw - w - margin, margin),
        Anchor::BottomLeft => (margin, fh - h - margin),
        Anchor::BottomRight => (fw - w - margin, fh - h - margin),
        Anchor::Center => (((fw - w) / 2.0).round(), ((fh - h) / 2.0).round()),
    };
    Rect { x: x.max(0.0), y: y.max(0.0), w, h }
}

/// Blends `src` onto `dest` with its top-left corner at `(x, y)` and extra `opacity`.
pub fn blend_image(dest: &mut RgbaImage, src: &RgbaImage, x: i64, y: i64, opacity: f32) {
    let opacity = opacity.clamp(0.0, 1.0);
    for (sx, sy, pixel) in src.enumerate_pixels() {
        let (dx, dy) = (x + i64::from(sx), y + i64::from(sy));
        if dx < 0 || dy < 0 || dx >= i64::from(dest.width()) || dy >= i64::from(dest.height()) {
            continue;
        }
        let a = f32::from(pixel.0[3]) / 255.0 * opacity;
        if a <= 0.0 {
            continue;
        }
        let d = dest.get_pixel_mut(dx as u32, dy as u32);
        for c in 0..3 {
            d.0[c] = (f32::from(pixel.0[c]) * a + f32::from(d.0[c]) * (1.0 - a)).round() as u8;
        }
        d.0[3] = (255.0 * a + f32::from(d.0[3]) * (1.0 - a)).round() as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Frame;

    fn project() -> Project {
        let mut p = Project::new(200, 100);
        for (i, delay) in [100, 100, 200].into_iter().enumerate() {
            p.frames.push(Frame { id: format!("f{i}"), file: String::new(), delay_ms: delay, time_ms: None });
        }
        p
    }

    #[test]
    fn progress_bar_fills_up_to_the_end_of_each_frame() {
        let mut p = project();
        assert!(p.progress_annotation(0).is_none());
        p.overlays.progress = Some(ProgressBar { bottom: true, height: 4.0, color: Rgba::RED });
        let width = |i| match p.progress_annotation(i).unwrap().shape {
            Shape::Rect { rect } => (rect.w, rect.y),
            _ => unreachable!(),
        };
        assert_eq!(width(0), (50.0, 96.0));
        assert_eq!(width(2), (200.0, 96.0));
    }

    #[test]
    fn watermark_goes_into_the_chosen_corner() {
        let wm = Watermark {
            file: String::new(),
            image_width: 100,
            image_height: 50,
            anchor: Anchor::BottomRight,
            size: 0.25,
            opacity: 1.0,
        };
        assert_eq!(watermark_rect(200, 100, &wm), Rect { x: 148.0, y: 73.0, w: 50.0, h: 25.0 });
        let tall = Watermark { image_width: 10, image_height: 100, size: 1.0, anchor: Anchor::Center, ..wm };
        assert_eq!(watermark_rect(200, 100, &tall), Rect { x: 95.0, y: 0.0, w: 10.0, h: 100.0 });
    }

    #[test]
    fn blending_respects_alpha_and_opacity() {
        let mut dest = RgbaImage::from_pixel(2, 1, image::Rgba([0, 0, 0, 255]));
        let src = RgbaImage::from_pixel(1, 1, image::Rgba([200, 100, 0, 255]));
        blend_image(&mut dest, &src, 1, 0, 0.5);
        assert_eq!(dest.get_pixel(1, 0).0, [100, 50, 0, 255]);
        assert_eq!(dest.get_pixel(0, 0).0, [0, 0, 0, 255]);
        blend_image(&mut dest, &src, 5, 5, 1.0); // outside: nothing happens
    }
}
