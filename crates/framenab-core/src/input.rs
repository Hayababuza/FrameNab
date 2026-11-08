//! Input recorded alongside a recording (`input.json` in the project folder): mouse positions,
//! clicks and key presses with their recording time. The editor turns them into effects
//! (cursor highlight, click ripples, key captions) that are burned into the result.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Annotation, Point, Project, Rect, Rgba, Shape, Style};

/// Mouse position at a recording time, in pixels of the recording.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CursorSample {
    #[ts(type = "number")]
    pub t: u64,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClickEvent {
    #[ts(type = "number")]
    pub t: u64,
    pub x: f32,
    pub y: f32,
    /// 0 = left, 1 = right, 2 = middle.
    pub button: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct KeyEvent {
    #[ts(type = "number")]
    pub t: u64,
    /// What to show, e.g. `Ctrl + C`.
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InputEvents {
    pub cursor: Vec<CursorSample>,
    pub clicks: Vec<ClickEvent>,
    pub keys: Vec<KeyEvent>,
}

impl InputEvents {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cursor.is_empty() && self.clicks.is_empty() && self.keys.is_empty()
    }

    /// Mouse position at time `t` (interpolated between samples).
    #[must_use]
    pub fn cursor_at(&self, t: u64) -> Option<Point> {
        let after = self.cursor.partition_point(|s| s.t <= t);
        match (after.checked_sub(1).map(|i| self.cursor[i]), self.cursor.get(after)) {
            (Some(a), Some(b)) if b.t > a.t => {
                let f = (t - a.t) as f32 / (b.t - a.t) as f32;
                Some(Point { x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f })
            }
            (Some(a), _) => Some(Point { x: a.x, y: a.y }),
            (None, Some(b)) => Some(Point { x: b.x, y: b.y }),
            (None, None) => None,
        }
    }
}

/// Which input effects are burned into the recording.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Effects {
    pub cursor: bool,
    pub clicks: bool,
    pub keys: bool,
}

impl Effects {
    #[must_use]
    pub fn any(self) -> bool {
        self.cursor || self.clicks || self.keys
    }
}

/// How long a click ripple stays visible.
pub const CLICK_MS: u64 = 500;
/// How long a key caption stays visible.
pub const KEY_MS: u64 = 1500;

/// Annotations for the input effects at recording time `t`, sized for a `width`×`height`
/// recording. The UI preview mirrors this (`src/lib/annotations/effects.ts`).
#[must_use]
pub fn effect_annotations(input: &InputEvents, effects: Effects, t: u64, width: u32, height: u32) -> Vec<Annotation> {
    let unit = (width.min(height) as f32 / 40.0).max(8.0);
    let style = |stroke: Rgba, width: f32, opacity: f32| Style { stroke, width, fill: None, opacity };
    let mut out = Vec::new();
    if effects.cursor
        && let Some(p) = input.cursor_at(t)
    {
        let r = unit * 1.6;
        out.push(Annotation {
            id: "fx-cursor".into(),
            frame_start: 0,
            frame_end: u32::MAX,
            shape: Shape::Ellipse { rect: Rect { x: p.x - r, y: p.y - r, w: 2.0 * r, h: 2.0 * r } },
            style: Style {
                fill: Some(Rgba { r: 255, g: 214, b: 0, a: 255 }),
                ..style(Rgba { r: 255, g: 214, b: 0, a: 255 }, 1.0, 0.35)
            },
        });
    }
    if effects.clicks {
        for (i, c) in input.clicks.iter().enumerate().filter(|(_, c)| c.t <= t && t < c.t + CLICK_MS) {
            let color = match c.button {
                1 => Rgba { r: 10, g: 132, b: 255, a: 255 },
                2 => Rgba { r: 52, g: 199, b: 89, a: 255 },
                _ => Rgba::RED,
            };
            out.push(Annotation {
                id: format!("fx-click-{i}"),
                frame_start: 0,
                frame_end: u32::MAX,
                shape: Shape::Click { at: Point { x: c.x, y: c.y } },
                style: style(color, unit * 0.5, 1.0),
            });
        }
    }
    if effects.keys {
        let recent: Vec<&str> =
            input.keys.iter().filter(|k| k.t <= t && t < k.t + KEY_MS).map(|k| k.label.as_str()).collect();
        if !recent.is_empty() {
            let font_size = unit * 1.4;
            let text = recent[recent.len().saturating_sub(4)..].join("  ");
            let approx_width = text.chars().count() as f32 * font_size * 0.6;
            out.push(Annotation {
                id: "fx-keys".into(),
                frame_start: 0,
                frame_end: u32::MAX,
                shape: Shape::Text {
                    at: Point { x: (width as f32 - approx_width) / 2.0, y: height as f32 - font_size * 2.6 },
                    text,
                    font_size,
                    background: Some(Rgba { r: 20, g: 20, b: 24, a: 220 }),
                },
                style: style(Rgba { r: 255, g: 255, b: 255, a: 255 }, 1.0, 1.0),
            });
        }
    }
    out
}

impl Project {
    /// Recording time of frame `index`: when it was captured, or (for frames without a
    /// timestamp) the sum of the previous delays.
    #[must_use]
    pub fn frame_time(&self, index: usize) -> u64 {
        self.frames
            .get(index)
            .and_then(|f| f.time_ms)
            .unwrap_or_else(|| self.frames.iter().take(index).map(|f| u64::from(f.delay_ms)).sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> InputEvents {
        InputEvents {
            cursor: vec![CursorSample { t: 0, x: 0.0, y: 0.0 }, CursorSample { t: 100, x: 100.0, y: 50.0 }],
            clicks: vec![ClickEvent { t: 200, x: 10.0, y: 20.0, button: 0 }],
            keys: vec![KeyEvent { t: 300, label: "Ctrl + C".into() }],
        }
    }

    #[test]
    fn cursor_is_interpolated_and_clamped() {
        let i = input();
        assert_eq!(i.cursor_at(50), Some(Point { x: 50.0, y: 25.0 }));
        assert_eq!(i.cursor_at(500), Some(Point { x: 100.0, y: 50.0 }));
        assert_eq!(InputEvents::default().cursor_at(5), None);
    }

    #[test]
    fn effects_appear_only_when_enabled_and_in_time() {
        let i = input();
        let all = Effects { cursor: true, clicks: true, keys: true };
        let kinds = |t| {
            effect_annotations(&i, all, t, 800, 600)
                .into_iter()
                .map(|a| match a.shape {
                    Shape::Ellipse { .. } => "cursor",
                    Shape::Click { .. } => "click",
                    Shape::Text { .. } => "keys",
                    _ => "?",
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(kinds(150), ["cursor"]);
        assert_eq!(kinds(250), ["cursor", "click"]);
        assert_eq!(kinds(350), ["cursor", "click", "keys"]);
        assert_eq!(kinds(900), ["cursor", "keys"]);
        assert!(effect_annotations(&i, Effects::default(), 350, 800, 600).is_empty());
    }
}
