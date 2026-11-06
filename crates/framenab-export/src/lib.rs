//! Export von Projekten: GIF (gifski), APNG, WebP, PNG-Sequenz und, wenn FFmpeg installiert
//! ist, MP4/WebM. FFmpeg wird nicht mitgeliefert.
//!
//! Phase 0: Formate, Optionen und FFmpeg-Erkennung. Die Encoder folgen in Phase 4.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Gif,
    Apng,
    Webp,
    PngSequence,
    Mp4,
    Webm,
}

impl ExportFormat {
    pub const ALL: [Self; 6] = [Self::Gif, Self::Apng, Self::Webp, Self::PngSequence, Self::Mp4, Self::Webm];

    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Gif => "gif",
            Self::Apng => "png",
            Self::Webp => "webp",
            Self::PngSequence => "zip",
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
        }
    }

    #[must_use]
    pub fn requires_ffmpeg(self) -> bool {
        matches!(self, Self::Mp4 | Self::Webm)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GifOptions {
    /// 1–100, an gifski durchgereicht.
    pub quality: u8,
    /// Verlustbehaftete Kompression für kleinere Dateien (gifski `lossy_quality`).
    pub lossy: Option<u8>,
    /// Feste Wiederholungen; `None` = Wert aus dem Projekt.
    pub loop_count: Option<u16>,
}

impl Default for GifOptions {
    fn default() -> Self {
        Self { quality: 90, lossy: None, loop_count: None }
    }
}

/// Sucht eine installierte FFmpeg-Programmdatei im `PATH`.
#[must_use]
pub fn find_ffmpeg() -> Option<PathBuf> {
    which::which("ffmpeg").ok()
}

/// Formate, die auf diesem System gerade exportierbar sind.
#[must_use]
pub fn available_formats() -> Vec<ExportFormat> {
    let ffmpeg = find_ffmpeg().is_some();
    ExportFormat::ALL.into_iter().filter(|f| ffmpeg || !f.requires_ffmpeg()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_ffmpeg_formats_always_available() {
        let formats = available_formats();
        for f in [ExportFormat::Gif, ExportFormat::Apng, ExportFormat::Webp, ExportFormat::PngSequence] {
            assert!(formats.contains(&f));
        }
    }

    #[test]
    fn format_serializes_camel_case() {
        assert_eq!(serde_json::to_string(&ExportFormat::PngSequence).unwrap(), "\"pngSequence\"");
    }
}
