//! Project model. A screenshot is a project with exactly one frame.
//!
//! All types are exported to TypeScript via `ts-rs` (`cargo test` → `src/lib/bindings/`),
//! so the UI preview and the export renderer share the same schema.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Current version of the `project.json` format.
pub const PROJECT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Project {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Frame>,
    pub annotations: Vec<Annotation>,
    /// Number of repetitions on export; 0 = infinite.
    #[serde(rename = "loop")]
    pub loop_count: u16,
    /// Counter for unique frame IDs.
    #[serde(default)]
    pub next_frame_id: u32,
    /// Input effects burned into the result (recordings only).
    #[serde(default)]
    pub effects: crate::Effects,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Frame {
    pub id: String,
    /// Path relative to the project folder, e.g. `frames/f000001.qoi`.
    pub file: String,
    pub delay_ms: u32,
    /// When the frame was captured (recording time, pauses excluded), to match it with the
    /// recorded input even after frames were deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub time_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// Rect with positive width/height (dragging to the left/top produces negative values).
    #[must_use]
    pub fn normalized(self) -> Self {
        let (x, w) = if self.w < 0.0 { (self.x + self.w, -self.w) } else { (self.x, self.w) };
        let (y, h) = if self.h < 0.0 { (self.y + self.h, -self.h) } else { (self.y, self.h) };
        Self { x, y, w, h }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const RED: Self = Self { r: 0xFF, g: 0x3B, b: 0x30, a: 0xFF };
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Style {
    pub stroke: Rgba,
    pub width: f32,
    pub fill: Option<Rgba>,
    pub opacity: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self { stroke: Rgba::RED, width: 4.0, fill: None, opacity: 1.0 }
    }
}

/// Geometry of an annotation. Serialized as `{ "type": "arrow", ... }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum Shape {
    Arrow {
        from: Point,
        to: Point,
    },
    Line {
        from: Point,
        to: Point,
    },
    Rect {
        rect: Rect,
    },
    Ellipse {
        rect: Rect,
    },
    Pen {
        points: Vec<Point>,
    },
    Highlighter {
        points: Vec<Point>,
    },
    Text {
        at: Point,
        text: String,
        font_size: f32,
        background: Option<Rgba>,
    },
    Counter {
        at: Point,
        value: u32,
    },
    /// Marks a mouse click: a filled circle with a ring around it.
    Click {
        at: Point,
    },
    Blur {
        rect: Rect,
        radius: f32,
    },
    Pixelate {
        rect: Rect,
        block_size: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Annotation {
    pub id: String,
    /// First frame (inclusive) on which the annotation is visible.
    pub frame_start: u32,
    /// Last frame (inclusive).
    pub frame_end: u32,
    pub shape: Shape,
    pub style: Style,
}

impl Point {
    #[must_use]
    pub fn offset(self, dx: f32, dy: f32) -> Self {
        Self { x: self.x + dx, y: self.y + dy }
    }
}

impl Rect {
    #[must_use]
    pub fn offset(self, dx: f32, dy: f32) -> Self {
        Self { x: self.x + dx, y: self.y + dy, ..self }
    }
}

impl Shape {
    /// Moves the shape by `(dx, dy)`.
    #[must_use]
    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        let mut shape = self.clone();
        match &mut shape {
            Self::Arrow { from, to } | Self::Line { from, to } => {
                *from = from.offset(dx, dy);
                *to = to.offset(dx, dy);
            }
            Self::Rect { rect } | Self::Ellipse { rect } | Self::Blur { rect, .. } | Self::Pixelate { rect, .. } => {
                *rect = rect.offset(dx, dy);
            }
            Self::Pen { points } | Self::Highlighter { points } => {
                points.iter_mut().for_each(|p| *p = p.offset(dx, dy));
            }
            Self::Text { at, .. } | Self::Counter { at, .. } | Self::Click { at } => *at = at.offset(dx, dy),
        }
        shape
    }
}

impl Annotation {
    #[must_use]
    pub fn visible_on(&self, frame_index: u32) -> bool {
        (self.frame_start..=self.frame_end).contains(&frame_index)
    }
}

impl Project {
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            version: PROJECT_VERSION,
            width,
            height,
            frames: Vec::new(),
            annotations: Vec::new(),
            loop_count: 0,
            next_frame_id: 0,
            effects: crate::Effects::default(),
        }
    }

    /// Total duration in milliseconds.
    #[must_use]
    pub fn duration_ms(&self) -> u64 {
        self.frames.iter().map(|f| u64::from(f.delay_ms)).sum()
    }

    /// Allocates a new, unique frame ID.
    pub fn allocate_frame_id(&mut self) -> String {
        self.next_frame_id += 1;
        format!("f{:06}", self.next_frame_id)
    }

    /// Annotations visible on frame `index`, in drawing order.
    pub fn annotations_on(&self, index: u32) -> impl Iterator<Item = &Annotation> {
        self.annotations.iter().filter(move |a| a.visible_on(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_serializes_with_type_tag() {
        let s = Shape::Arrow { from: Point { x: 1.0, y: 2.0 }, to: Point::default() };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["type"], "arrow");
        assert_eq!(json["from"]["y"], 2.0);

        let t = Shape::Text { at: Point::default(), text: "Hi".into(), font_size: 12.0, background: None };
        assert_eq!(serde_json::to_value(&t).unwrap()["fontSize"], 12.0);
    }

    #[test]
    fn project_roundtrip_uses_loop_key() {
        let mut p = Project::new(10, 10);
        p.loop_count = 3;
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"loop\":3"));
        assert_eq!(serde_json::from_str::<Project>(&json).unwrap(), p);
    }

    #[test]
    fn shapes_translate() {
        let pen = Shape::Pen { points: vec![Point { x: 1.0, y: 1.0 }, Point { x: 2.0, y: 3.0 }] };
        assert_eq!(
            pen.translated(10.0, -1.0),
            Shape::Pen { points: vec![Point { x: 11.0, y: 0.0 }, Point { x: 12.0, y: 2.0 }] }
        );
        let blur = Shape::Blur { rect: Rect { x: 0.0, y: 0.0, w: 5.0, h: 5.0 }, radius: 3.0 };
        assert_eq!(
            blur.translated(1.0, 2.0),
            Shape::Blur { rect: Rect { x: 1.0, y: 2.0, w: 5.0, h: 5.0 }, radius: 3.0 }
        );
    }

    #[test]
    fn rect_normalizes_negative_size() {
        let r = Rect { x: 10.0, y: 10.0, w: -4.0, h: -6.0 }.normalized();
        assert_eq!(r, Rect { x: 6.0, y: 4.0, w: 4.0, h: 6.0 });
    }
}
