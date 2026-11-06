//! Renders one of every annotation kind: `cargo run -p framenab-render --example showcase [out.png]`

use framenab_core::{Annotation, Point, Rect, Rgba, Shape, Style};
use image::RgbaImage;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "showcase.png".into());
    // A checkerboard with stripes, so blur and pixelate are visible.
    let mut image = RgbaImage::from_fn(640, 360, |x, y| {
        let v = if ((x / 20) + (y / 20)) % 2 == 0 { 235 } else { 200 };
        if x % 7 == 0 { image::Rgba([40, 90, 160, 255]) } else { image::Rgba([v, v, v, 255]) }
    });
    let p = |x: f32, y: f32| Point { x, y };
    let blue = Rgba { r: 10, g: 132, b: 255, a: 255 };
    let style = |stroke: Rgba, width: f32| Style { stroke, width, fill: None, opacity: 1.0 };
    let shapes = [
        (Shape::Arrow { from: p(30.0, 40.0), to: p(200.0, 90.0) }, style(Rgba::RED, 4.0)),
        (Shape::Line { from: p(30.0, 120.0), to: p(200.0, 120.0) }, style(blue, 2.0)),
        (Shape::Rect { rect: Rect { x: 240.0, y: 30.0, w: 150.0, h: 90.0 } }, style(Rgba::RED, 4.0)),
        (Shape::Ellipse { rect: Rect { x: 420.0, y: 30.0, w: 180.0, h: 90.0 } }, style(blue, 4.0)),
        (
            Shape::Pen {
                points: (0..40).map(|i| p(30.0 + i as f32 * 4.0, 180.0 + (i as f32 / 3.0).sin() * 15.0)).collect(),
            },
            style(Rgba::RED, 4.0),
        ),
        (
            Shape::Highlighter { points: vec![p(30.0, 240.0), p(200.0, 240.0)] },
            style(Rgba { r: 255, g: 204, b: 0, a: 255 }, 8.0),
        ),
        (
            Shape::Text {
                at: p(240.0, 150.0),
                text: "Hallo FrameNab\nzweite Zeile".into(),
                font_size: 28.0,
                background: None,
            },
            style(Rgba::RED, 4.0),
        ),
        (Shape::Counter { at: p(270.0, 280.0), value: 1 }, style(Rgba::RED, 4.0)),
        (Shape::Counter { at: p(330.0, 280.0), value: 12 }, style(blue, 4.0)),
        (
            Shape::Pixelate { rect: Rect { x: 420.0, y: 150.0, w: 180.0, h: 80.0 }, block_size: 12 },
            style(Rgba::RED, 4.0),
        ),
        (Shape::Blur { rect: Rect { x: 420.0, y: 250.0, w: 180.0, h: 80.0 }, radius: 10.0 }, style(Rgba::RED, 4.0)),
    ];
    let annotations: Vec<Annotation> = shapes
        .into_iter()
        .enumerate()
        .map(|(i, (shape, style))| Annotation { id: format!("a{i}"), frame_start: 0, frame_end: 0, shape, style })
        .collect();
    framenab_render::render_annotations(&mut image, &annotations).expect("render");
    image.save(&out).expect("save");
    println!("saved to {out}");
}
