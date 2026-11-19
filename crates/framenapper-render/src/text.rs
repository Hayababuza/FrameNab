//! Text rendering with cosmic-text, using the system's default sans-serif font (the same one
//! the webview's `sans-serif` resolves to, so the preview and the result match closely).

use std::sync::{Mutex, OnceLock};

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use framenapper_core::{Point, Rgba};
use image::RgbaImage;

/// Line height relative to the font size; the UI uses the same factor.
pub const LINE_HEIGHT: f32 = 1.25;

struct Fonts {
    system: FontSystem,
    cache: SwashCache,
}

fn fonts() -> &'static Mutex<Fonts> {
    static FONTS: OnceLock<Mutex<Fonts>> = OnceLock::new();
    FONTS.get_or_init(|| Mutex::new(Fonts { system: FontSystem::new(), cache: SwashCache::new() }))
}

/// Loads the system fonts in advance (this takes a moment), so the first text renders quickly.
pub fn warm_up() {
    drop(fonts().lock());
}

fn layout(fonts: &mut Fonts, text: &str, size: f32, bold: bool) -> Buffer {
    let mut buffer = Buffer::new(&mut fonts.system, Metrics::new(size, size * LINE_HEIGHT));
    let attrs = Attrs::new().family(Family::SansSerif).weight(if bold { Weight::BOLD } else { Weight::NORMAL });
    let mut borrowed = buffer.borrow_with(&mut fonts.system);
    borrowed.set_size(None, None);
    borrowed.set_text(text, &attrs, Shaping::Advanced, None);
    borrowed.shape_until_scroll(false);
    buffer
}

fn extent(buffer: &Buffer) -> (f32, f32) {
    buffer
        .layout_runs()
        .fold((0.0f32, 0.0f32), |(w, h), run| (w.max(run.line_w), h.max(run.line_top + run.line_height)))
}

/// Width and height of `text` at `size`.
#[must_use]
pub fn measure(text: &str, size: f32, bold: bool) -> (f32, f32) {
    let mut fonts = fonts().lock().expect("font lock");
    extent(&layout(&mut fonts, text, size, bold))
}

/// Draws `text` with its top-left corner at `at`.
pub fn draw(image: &mut RgbaImage, at: Point, text: &str, size: f32, color: Rgba, opacity: f32, bold: bool) {
    let mut guard = fonts().lock().expect("font lock");
    let fonts = &mut *guard;
    let mut buffer = layout(fonts, text, size, bold);
    let (ox, oy) = (at.x.round() as i32, at.y.round() as i32);
    let alpha = opacity.clamp(0.0, 1.0) * f32::from(color.a) / 255.0;
    buffer.draw(&mut fonts.system, &mut fonts.cache, Color::rgb(color.r, color.g, color.b), |x, y, w, h, c| {
        let coverage = f32::from(c.a()) / 255.0 * alpha;
        if coverage <= 0.0 {
            return;
        }
        for py in y..y + h as i32 {
            for px in x..x + w as i32 {
                blend(image, ox + px, oy + py, [c.r(), c.g(), c.b()], coverage);
            }
        }
    });
}

/// Source-over blend of one pixel.
pub(crate) fn blend(image: &mut RgbaImage, x: i32, y: i32, rgb: [u8; 3], alpha: f32) {
    if x < 0 || y < 0 || x as u32 >= image.width() || y as u32 >= image.height() {
        return;
    }
    let dst = image.get_pixel_mut(x as u32, y as u32);
    let mix = |s: u8, d: u8| (f32::from(s) * alpha + f32::from(d) * (1.0 - alpha)).round() as u8;
    dst.0 = [
        mix(rgb[0], dst.0[0]),
        mix(rgb[1], dst.0[1]),
        mix(rgb[2], dst.0[2]),
        (alpha * 255.0 + f32::from(dst.0[3]) * (1.0 - alpha)).round() as u8,
    ];
}
