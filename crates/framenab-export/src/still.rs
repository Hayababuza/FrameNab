//! Saving single images (screenshots): formats, file name templates and unique paths.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Local, Timelike};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use serde::{Deserialize, Serialize};

pub const DEFAULT_TEMPLATE: &str = "FrameNab_{yyyy-MM-dd_HH-mm-ss}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StillFormat {
    #[default]
    Png,
    Jpeg,
    Webp,
}

impl StillFormat {
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }

    /// Format for a file extension (case-insensitive), e.g. from a "Save as" dialog.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("Could not write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Could not encode the image: {0}")]
    Encode(#[from] image::ImageError),
}

/// Expands `{…}` placeholders with date/time tokens: `yyyy`, `yy`, `MM`, `dd`, `HH`, `mm`, `ss`.
/// Text outside braces is kept; characters that are invalid in file names are replaced by `_`.
#[must_use]
pub fn expand_template(template: &str, now: &DateTime<Local>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        out.push_str(&expand_tokens(&rest[open + 1..open + close], now));
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    let name: String =
        out.chars().map(|c| if r#"/\:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let name = name.trim();
    if name.is_empty() { "FrameNab".to_owned() } else { name.to_owned() }
}

/// [`expand_template`] with the current local time.
#[must_use]
pub fn file_name_now(template: &str) -> String {
    expand_template(template, &Local::now())
}

fn expand_tokens(pattern: &str, now: &DateTime<Local>) -> String {
    let tokens: [(&str, String); 7] = [
        ("yyyy", format!("{:04}", now.year())),
        ("yy", format!("{:02}", now.year() % 100)),
        ("MM", format!("{:02}", now.month())),
        ("dd", format!("{:02}", now.day())),
        ("HH", format!("{:02}", now.hour())),
        ("mm", format!("{:02}", now.minute())),
        ("ss", format!("{:02}", now.second())),
    ];
    let mut out = String::new();
    let mut rest = pattern;
    'outer: while !rest.is_empty() {
        for (token, value) in &tokens {
            if let Some(after) = rest.strip_prefix(token) {
                out.push_str(value);
                rest = after;
                continue 'outer;
            }
        }
        let mut chars = rest.chars();
        out.extend(chars.next());
        rest = chars.as_str();
    }
    out
}

/// `dir/name.ext`, or `dir/name_2.ext`, `dir/name_3.ext` … if the file already exists.
#[must_use]
pub fn unique_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{name}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{name}_{n}.{ext}"))).find(|p| !p.exists()).expect("unbounded range")
}

/// Encodes and writes `image`. JPEG has no alpha channel, so transparency is flattened on white.
pub fn save_still(image: &RgbaImage, path: &Path, format: StillFormat) -> Result<(), SaveError> {
    let io = |source| SaveError::Io { path: path.to_owned(), source };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let writer = BufWriter::new(File::create(path).map_err(io)?);
    let (w, h) = image.dimensions();
    match format {
        StillFormat::Png => PngEncoder::new(writer).write_image(image.as_raw(), w, h, ExtendedColorType::Rgba8)?,
        StillFormat::Webp => {
            WebPEncoder::new_lossless(writer).write_image(image.as_raw(), w, h, ExtendedColorType::Rgba8)?;
        }
        StillFormat::Jpeg => {
            let rgb: Vec<u8> = image
                .pixels()
                .flat_map(|p| {
                    let a = u16::from(p.0[3]);
                    p.0[..3]
                        .iter()
                        .map(move |&c| ((u16::from(c) * a + 255 * (255 - a)) / 255) as u8)
                        .collect::<Vec<_>>()
                })
                .collect();
            JpegEncoder::new_with_quality(writer, 90).write_image(&rgb, w, h, ExtendedColorType::Rgb8)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 3, 7, 9, 5, 2).unwrap()
    }

    #[test]
    fn expands_templates() {
        assert_eq!(expand_template(DEFAULT_TEMPLATE, &now()), "FrameNab_2026-03-07_09-05-02");
        assert_eq!(expand_template("shot {dd.MM.yy} at {HH}h", &now()), "shot 07.03.26 at 09h");
        assert_eq!(expand_template("no braces", &now()), "no braces");
        assert_eq!(expand_template("bad/chars:{yyyy", &now()), "bad_chars_{yyyy");
        assert_eq!(expand_template("  ", &now()), "FrameNab");
    }

    #[test]
    fn unique_paths_and_formats() {
        let dir = tempfile::tempdir().unwrap();
        let image = RgbaImage::from_pixel(4, 3, image::Rgba([10, 20, 30, 128]));
        for format in [StillFormat::Png, StillFormat::Jpeg, StillFormat::Webp] {
            let path = unique_path(dir.path(), "shot", format.extension());
            save_still(&image, &path, format).unwrap();
            let loaded = image::open(&path).unwrap();
            assert_eq!((loaded.width(), loaded.height()), (4, 3));
        }
        assert_eq!(unique_path(dir.path(), "shot", "png"), dir.path().join("shot_2.png"));
        assert_eq!(StillFormat::from_extension("JPEG"), Some(StillFormat::Jpeg));
    }
}
