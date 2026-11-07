//! Renders annotations into frames, for export and for screenshots (clipboard/file).
//!
//! The UI draws live on an HTML canvas (`src/lib/annotations/render.ts`); this renderer is the
//! source of truth for the result. Sizes derived from the stroke width (counter radius, text
//! padding, highlighter width) are defined here and mirrored in the UI.

pub mod text;

use framenab_core::{Annotation, Point, Project, Rect, Rgba, Shape, Style};
use image::RgbaImage;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("Invalid image size {0}x{1}")]
    InvalidSize(u32, u32),
}

/// Radius of a numbered counter marker for a given stroke width.
#[must_use]
pub fn counter_radius(width: f32) -> f32 {
    10.0 + width * 2.0
}

/// Outer radius of a click marker for a given stroke width.
#[must_use]
pub fn click_radius(width: f32) -> f32 {
    14.0 + width * 3.0
}

/// Padding around text that has a background.
#[must_use]
pub fn text_padding(font_size: f32) -> f32 {
    (font_size * 0.2).round()
}

/// Renders every annotation visible on `frame_index` onto a copy of `base`.
pub fn render_frame(project: &Project, frame_index: u32, base: &RgbaImage) -> Result<RgbaImage, RenderError> {
    let mut image = base.clone();
    for annotation in project.annotations_on(frame_index) {
        draw(&mut image, annotation)?;
    }
    Ok(image)
}

/// Draws all annotations in order into `image`.
pub fn render_annotations(image: &mut RgbaImage, annotations: &[Annotation]) -> Result<(), RenderError> {
    annotations.iter().try_for_each(|a| draw(image, a))
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
            blur(image, *rect, *radius);
            return Ok(());
        }
        Shape::Text { at, text, font_size, background } => {
            if let Some(bg) = background {
                let (w, h) = text::measure(text, *font_size, false);
                let pad = text_padding(*font_size);
                let rect = Rect { x: at.x - pad, y: at.y - pad, w: w + 2.0 * pad, h: h + 2.0 * pad };
                fill_rect(image, rect, *bg, style.opacity)?;
            }
            text::draw(image, *at, text, *font_size, style.stroke, style.opacity, false);
            return Ok(());
        }
        Shape::Counter { at, value } => {
            draw_counter(image, *at, *value, style)?;
            return Ok(());
        }
        Shape::Click { at } => {
            draw_click(image, *at, style)?;
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

fn fill_rect(image: &mut RgbaImage, rect: Rect, color: Rgba, opacity: f32) -> Result<(), RenderError> {
    let (w, h) = image.dimensions();
    let mut pixmap = Pixmap::new(w, h).ok_or(RenderError::InvalidSize(w, h))?;
    let r = rect.normalized();
    fill_path(&mut pixmap, tiny_skia::Rect::from_xywh(r.x, r.y, r.w, r.h).map(PathBuilder::from_rect), color, opacity);
    composite(image, &pixmap);
    Ok(())
}

/// A filled circle in the stroke color with the number in white, centered on `at`.
fn draw_counter(image: &mut RgbaImage, at: Point, value: u32, style: &Style) -> Result<(), RenderError> {
    let (w, h) = image.dimensions();
    let mut pixmap = Pixmap::new(w, h).ok_or(RenderError::InvalidSize(w, h))?;
    let radius = counter_radius(style.width);
    fill_path(&mut pixmap, PathBuilder::from_circle(at.x, at.y, radius), style.stroke, style.opacity);
    composite(image, &pixmap);

    let label = value.to_string();
    let size = radius * 1.1;
    let (tw, th) = text::measure(&label, size, true);
    let white = Rgba { r: 255, g: 255, b: 255, a: 255 };
    text::draw(image, Point { x: at.x - tw / 2.0, y: at.y - th / 2.0 }, &label, size, white, style.opacity, true);
    Ok(())
}

/// Click marker: a translucent filled circle and a ring in the stroke color.
fn draw_click(image: &mut RgbaImage, at: Point, style: &Style) -> Result<(), RenderError> {
    let (w, h) = image.dimensions();
    let mut pixmap = Pixmap::new(w, h).ok_or(RenderError::InvalidSize(w, h))?;
    let radius = click_radius(style.width);
    fill_path(&mut pixmap, PathBuilder::from_circle(at.x, at.y, radius * 0.6), style.stroke, style.opacity * 0.45);
    let ring = Style { width: (style.width * 0.75).max(2.0), ..style.clone() };
    stroke_path(&mut pixmap, PathBuilder::from_circle(at.x, at.y, radius), &ring);
    composite(image, &pixmap);
    Ok(())
}

/// Clamps a rect to the image and returns `(x, y, w, h)` in whole pixels.
fn pixel_rect(image: &RgbaImage, rect: Rect) -> Option<(u32, u32, u32, u32)> {
    let r = rect.normalized();
    let (w, h) = image.dimensions();
    let x0 = (r.x.max(0.0) as u32).min(w);
    let y0 = (r.y.max(0.0) as u32).min(h);
    let x1 = ((r.x + r.w).ceil().max(0.0) as u32).min(w);
    let y1 = ((r.y + r.h).ceil().max(0.0) as u32).min(h);
    (x1 > x0 && y1 > y0).then(|| (x0, y0, x1 - x0, y1 - y0))
}

/// Gaussian-like blur of an area (`radius` is the sigma in pixels).
pub fn blur(image: &mut RgbaImage, rect: Rect, radius: f32) {
    let Some((x, y, w, h)) = pixel_rect(image, rect) else { return };
    let region = image::imageops::crop_imm(image, x, y, w, h).to_image();
    let blurred = image::imageops::fast_blur(&region, radius.max(1.0));
    image::imageops::replace(image, &blurred, i64::from(x), i64::from(y));
}

/// Pixelates an area in blocks of size `block`.
pub fn pixelate(image: &mut RgbaImage, rect: Rect, block: u32) {
    let block = block.max(2);
    let Some((x0, y0, w, h)) = pixel_rect(image, rect) else { return };
    let (x1, y1) = (x0 + w, y0 + h);

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
    fn blur_smooths_edges_inside_the_area_only() {
        let mut img = RgbaImage::from_fn(20, 10, |x, _| image::Rgba(if x < 10 { [0, 0, 0, 255] } else { [255; 4] }));
        blur(&mut img, Rect { x: 5.0, y: 0.0, w: 10.0, h: 10.0 }, 3.0);
        let mid = img.get_pixel(10, 5).0[0];
        assert!(mid > 20 && mid < 235, "edge should be blurred, got {mid}");
        assert_eq!(img.get_pixel(0, 5).0, [0, 0, 0, 255]);
        assert_eq!(img.get_pixel(19, 5).0, [255; 4]);
    }

    #[test]
    fn text_and_counter_draw_pixels() {
        let base = RgbaImage::from_pixel(120, 60, image::Rgba([255, 255, 255, 255]));
        let mut img = base.clone();
        let text = annotation(Shape::Text {
            at: Point { x: 5.0, y: 5.0 },
            text: "Hi".into(),
            font_size: 24.0,
            background: None,
        });
        draw(&mut img, &text).unwrap();
        assert_ne!(img, base, "text should change pixels");

        let mut img = base.clone();
        draw(&mut img, &annotation(Shape::Counter { at: Point { x: 60.0, y: 30.0 }, value: 3 })).unwrap();
        // Ring of the circle is red, far corner untouched.
        assert_eq!(img.get_pixel(60 - 15, 30).0, [0xFF, 0x3B, 0x30, 0xFF]);
        assert_eq!(img.get_pixel(0, 0).0, [255; 4]);
    }

    #[test]
    fn click_marker_has_ring_and_translucent_center() {
        let mut img = RgbaImage::from_pixel(100, 100, image::Rgba([255, 255, 255, 255]));
        draw(&mut img, &annotation(Shape::Click { at: Point { x: 50.0, y: 50.0 } })).unwrap();
        let ring = img.get_pixel(50 + click_radius(4.0) as u32, 50).0;
        assert_eq!(ring[1], 0x3B, "ring in the stroke color: {ring:?}");
        let center = img.get_pixel(50, 50).0;
        assert!(center[1] > 0x3B && center[1] < 255, "center is a blend: {center:?}");
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
