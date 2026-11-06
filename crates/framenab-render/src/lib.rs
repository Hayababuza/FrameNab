//! Renders annotations into frames, for export and for screenshots (clipboard/file).
//!
//! The UI draws live on an HTML canvas; this renderer is the source of truth for the exported
//! result. Text (cosmic-text) and blur follow in phase 1.

use framenab_core::{Annotation, Point, Project, Rect, Rgba, Shape, Style};
use image::RgbaImage;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("Invalid image size {0}x{1}")]
    InvalidSize(u32, u32),
}

/// Renders every annotation visible on `frame_index` onto a copy of `base`.
pub fn render_frame(project: &Project, frame_index: u32, base: &RgbaImage) -> Result<RgbaImage, RenderError> {
    let mut image = base.clone();
    for annotation in project.annotations_on(frame_index) {
        draw(&mut image, annotation)?;
    }
    Ok(image)
}

/// Draws a single annotation directly into `image`.
pub fn draw(image: &mut RgbaImage, annotation: &Annotation) -> Result<(), RenderError> {
    let style = &annotation.style;
    match &annotation.shape {
        Shape::Pixelate { rect, block_size } => {
            pixelate(image, *rect, *block_size);
            return Ok(());
        }
        Shape::Blur { rect, radius } => {
            // Until the real Gaussian blur lands (phase 1): heavy pixelation as a safe cover.
            pixelate(image, *rect, radius.max(4.0) as u32);
            return Ok(());
        }
        Shape::Text { .. } | Shape::Counter { .. } => {
            // TODO(phase 1): text rendering with cosmic-text.
            return Ok(());
        }
        _ => {}
    }

    let (w, h) = image.dimensions();
    let mut pixmap = Pixmap::new(w, h).ok_or(RenderError::InvalidSize(w, h))?;
    // RgbaImage is not premultiplied; draw annotations onto an empty layer and then alpha-blend
    // it over the image.
    match &annotation.shape {
        Shape::Line { from, to } => stroke_path(&mut pixmap, line(*from, *to), style),
        Shape::Arrow { from, to } => {
            stroke_path(&mut pixmap, line(*from, *to), style);
            fill_path(&mut pixmap, arrow_head(*from, *to, style.width), style.stroke, style.opacity);
        }
        Shape::Rect { rect } => {
            let r = rect.normalized();
            if let Some(path) = tiny_skia::Rect::from_xywh(r.x, r.y, r.w, r.h).map(PathBuilder::from_rect) {
                if let Some(fill) = style.fill {
                    fill_path(&mut pixmap, Some(path.clone()), fill, style.opacity);
                }
                stroke_path(&mut pixmap, Some(path), style);
            }
        }
        Shape::Ellipse { rect } => {
            let r = rect.normalized();
            let path = tiny_skia::Rect::from_xywh(r.x, r.y, r.w, r.h).and_then(PathBuilder::from_oval);
            if let Some(fill) = style.fill {
                fill_path(&mut pixmap, path.clone(), fill, style.opacity);
            }
            stroke_path(&mut pixmap, path, style);
        }
        Shape::Pen { points } => stroke_path(&mut pixmap, polyline(points), style),
        Shape::Highlighter { points } => {
            let marker = Style { opacity: style.opacity * 0.4, width: style.width * 3.0, ..style.clone() };
            stroke_path(&mut pixmap, polyline(points), &marker);
        }
        _ => unreachable!("handled above"),
    }
    composite(image, &pixmap);
    Ok(())
}

fn paint(color: Rgba, opacity: f32) -> Paint<'static> {
    let mut paint = Paint::default();
    let a = (f32::from(color.a) * opacity.clamp(0.0, 1.0)).round() as u8;
    paint.set_color_rgba8(color.r, color.g, color.b, a);
    paint.anti_alias = true;
    paint
}

fn stroke_path(pixmap: &mut Pixmap, path: Option<tiny_skia::Path>, style: &Style) {
    let Some(path) = path else { return };
    let stroke = Stroke {
        width: style.width.max(0.5),
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint(style.stroke, style.opacity), &stroke, Transform::identity(), None);
}

fn fill_path(pixmap: &mut Pixmap, path: Option<tiny_skia::Path>, color: Rgba, opacity: f32) {
    let Some(path) = path else { return };
    pixmap.fill_path(&path, &paint(color, opacity), FillRule::Winding, Transform::identity(), None);
}

fn line(from: Point, to: Point) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(from.x, from.y);
    pb.line_to(to.x, to.y);
    pb.finish()
}

fn polyline(points: &[Point]) -> Option<tiny_skia::Path> {
    let (first, rest) = points.split_first()?;
    let mut pb = PathBuilder::new();
    pb.move_to(first.x, first.y);
    if rest.is_empty() {
        pb.line_to(first.x + 0.01, first.y); // single point → visible dot
    }
    for p in rest {
        pb.line_to(p.x, p.y);
    }
    pb.finish()
}

/// Filled arrow head at the end point `to`. Its size scales with the stroke width.
fn arrow_head(from: Point, to: Point, width: f32) -> Option<tiny_skia::Path> {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let len = dx.hypot(dy);
    if len < f32::EPSILON {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    let size = (width * 4.0).max(10.0);
    let (bx, by) = (to.x - ux * size, to.y - uy * size);
    let (px, py) = (-uy * size * 0.5, ux * size * 0.5);
    let mut pb = PathBuilder::new();
    pb.move_to(to.x, to.y);
    pb.line_to(bx + px, by + py);
    pb.line_to(bx - px, by - py);
    pb.close();
    pb.finish()
}

/// Composites the (premultiplied) annotation layer over the image using source-over.
fn composite(image: &mut RgbaImage, layer: &Pixmap) {
    for (dst, src) in image.pixels_mut().zip(layer.pixels()) {
        let a = u32::from(src.alpha());
        if a == 0 {
            continue;
        }
        let inv = 255 - a;
        let blend = |s: u8, d: u8| ((u32::from(s) * 255 + u32::from(d) * inv + 127) / 255) as u8;
        dst.0 = [
            blend(src.red(), dst.0[0]),
            blend(src.green(), dst.0[1]),
            blend(src.blue(), dst.0[2]),
            (a + (u32::from(dst.0[3]) * inv + 127) / 255) as u8,
        ];
    }
}

/// Pixelates an area in blocks of size `block`.
pub fn pixelate(image: &mut RgbaImage, rect: Rect, block: u32) {
    let block = block.max(2);
    let r = rect.normalized();
    let (w, h) = image.dimensions();
    let x0 = (r.x.max(0.0) as u32).min(w);
    let y0 = (r.y.max(0.0) as u32).min(h);
    let x1 = ((r.x + r.w).ceil().max(0.0) as u32).min(w);
    let y1 = ((r.y + r.h).ceil().max(0.0) as u32).min(h);

    for by in (y0..y1).step_by(block as usize) {
        for bx in (x0..x1).step_by(block as usize) {
            let (ex, ey) = ((bx + block).min(x1), (by + block).min(y1));
            let mut sum = [0u64; 4];
            let mut n = 0u64;
            for y in by..ey {
                for x in bx..ex {
                    for (s, c) in sum.iter_mut().zip(image.get_pixel(x, y).0) {
                        *s += u64::from(c);
                    }
                    n += 1;
                }
            }
            let avg = image::Rgba(sum.map(|s| (s / n.max(1)) as u8));
            for y in by..ey {
                for x in bx..ex {
                    image.put_pixel(x, y, avg);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn annotation(shape: Shape) -> Annotation {
        Annotation { id: "a".into(), frame_start: 0, frame_end: 0, shape, style: Style::default() }
    }

    #[test]
    fn rect_stroke_hits_edge_but_not_center() {
        let mut img = RgbaImage::from_pixel(40, 40, image::Rgba([255, 255, 255, 255]));
        let a = annotation(Shape::Rect { rect: Rect { x: 10.0, y: 10.0, w: 20.0, h: 20.0 } });
        draw(&mut img, &a).unwrap();
        assert_eq!(img.get_pixel(10, 20).0, [0xFF, 0x3B, 0x30, 0xFF]);
        assert_eq!(img.get_pixel(20, 20).0, [255, 255, 255, 255]);
    }

    #[test]
    fn arrow_draws_head_at_target() {
        let mut img = RgbaImage::from_pixel(100, 20, image::Rgba([0, 0, 0, 255]));
        let a = annotation(Shape::Arrow { from: Point { x: 5.0, y: 10.0 }, to: Point { x: 90.0, y: 10.0 } });
        draw(&mut img, &a).unwrap();
        assert_ne!(img.get_pixel(85, 10).0, [0, 0, 0, 255]);
    }

    #[test]
    fn pixelate_averages_blocks() {
        let mut img = RgbaImage::from_fn(4, 4, |x, _| image::Rgba([if x % 2 == 0 { 0 } else { 200 }, 0, 0, 255]));
        pixelate(&mut img, Rect { x: 0.0, y: 0.0, w: 4.0, h: 4.0 }, 2);
        assert_eq!(img.get_pixel(0, 0).0[0], 100);
        assert_eq!(img.get_pixel(3, 3).0[0], 100);
    }

    #[test]
    fn render_frame_respects_frame_range() {
        let mut p = Project::new(10, 10);
        let mut a = annotation(Shape::Rect { rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 } });
        a.frame_start = 1;
        a.frame_end = 1;
        p.annotations.push(a);
        let base = RgbaImage::from_pixel(10, 10, image::Rgba([1, 2, 3, 255]));
        assert_eq!(render_frame(&p, 0, &base).unwrap(), base);
        assert_ne!(render_frame(&p, 1, &base).unwrap(), base);
    }
}
